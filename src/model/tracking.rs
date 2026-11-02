use std::collections::{HashMap, HashSet};

use super::{ProcessForest, ProcessIdentity, ProcessRow};

/// Runtime membership with explicit root-name provenance. Never persisted in profiles.
#[derive(Debug, Clone, Default)]
pub(crate) struct TrackingScope {
    members: HashMap<ProcessIdentity, HashSet<String>>,
}

impl TrackingScope {
    pub(crate) fn update(&mut self, rows: &[ProcessRow], roots: &HashSet<String>) {
        self.members.retain(|identity, sources| {
            sources.retain(|name| roots.contains(name));
            let name = identity.name.to_ascii_lowercase();
            if roots.contains(&name) {
                sources.insert(name);
            }
            !sources.is_empty()
        });
        let forest = ProcessForest::build(rows, 0..rows.len(), None);
        // Parent-first, iterative traversal also handles deep trees and broken cycles.
        for node in forest.visible_rows(rows, None, &HashSet::new()) {
            let row = &rows[node.row_index];
            let id = ProcessIdentity::from_row(row);
            let mut sources = self.members.get(&id).cloned().unwrap_or_default();
            let name = row.name.to_ascii_lowercase();
            if roots.contains(&name) {
                sources.insert(name);
            }
            if let Some(parent_index) = forest.parent_row_index(node.row_index) {
                let parent = &rows[parent_index];
                // A Windows parent PID can outlive its owner and be recycled. Unknown
                // start times cannot establish a safe lifetime edge.
                if matches!((parent.start_time, row.start_time), (Some(p), Some(c)) if p <= c)
                    && let Some(inherited) = self.members.get(&ProcessIdentity::from_row(parent))
                {
                    sources.extend(inherited.iter().cloned());
                }
            }
            if !sources.is_empty() {
                self.members.insert(id, sources);
            }
        }
    }

    pub(crate) fn contains(&self, identity: &ProcessIdentity) -> bool {
        self.members.contains_key(identity)
    }

    pub(crate) fn identities(&self) -> HashSet<ProcessIdentity> {
        self.members.keys().cloned().collect()
    }

    pub(crate) fn retain(&mut self, identities: &HashSet<ProcessIdentity>) {
        self.members
            .retain(|identity, _| identities.contains(identity));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: u32, parent: Option<u32>, name: &str, start: u64) -> ProcessRow {
        ProcessRow {
            pid,
            parent_pid: parent,
            name: name.into(),
            start_time: Some(start),
            ..Default::default()
        }
    }
    fn roots(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }
    #[test]
    fn descendants_are_identity_scoped_and_survive_parent_exit() {
        let parent = row(1, None, "root.exe", 1);
        let child = row(2, Some(1), "worker.exe", 2);
        let unrelated = row(3, None, "worker.exe", 3);
        let mut scope = TrackingScope::default();
        let roots = roots(&["root.exe"]);
        scope.update(&[child.clone(), unrelated.clone(), parent], &roots);
        assert!(scope.contains(&ProcessIdentity::from_row(&child)));
        assert!(!scope.contains(&ProcessIdentity::from_row(&unrelated)));
        let grandchild = row(4, Some(2), "helper.exe", 4);
        scope.update(&[grandchild.clone(), child.clone()], &roots);
        assert!(scope.contains(&ProcessIdentity::from_row(&grandchild)));
        let reused = row(2, Some(99), "worker.exe", 5);
        scope.update(std::slice::from_ref(&reused), &roots);
        assert!(!scope.contains(&ProcessIdentity::from_row(&reused)));
    }
    #[test]
    fn overlapping_roots_can_be_removed_independently() {
        let rows = [
            row(1, None, "root.exe", 1),
            row(2, Some(1), "worker.exe", 2),
            row(3, Some(2), "helper.exe", 3),
        ];
        let mut scope = TrackingScope::default();
        scope.update(&rows, &roots(&["root.exe", "worker.exe"]));
        scope.update(&rows[2..], &roots(&["worker.exe"]));
        assert!(scope.contains(&ProcessIdentity::from_row(&rows[2])));
        scope.update(&rows[2..], &HashSet::new());
        assert!(scope.identities().is_empty());
    }
    #[test]
    fn explicit_name_preserves_an_exited_inherited_identity() {
        let rows = [
            row(1, None, "root.exe", 1),
            row(2, Some(1), "worker.exe", 2),
        ];
        let mut scope = TrackingScope::default();
        scope.update(&rows, &roots(&["root.exe"]));
        scope.update(&[], &roots(&["worker.exe"]));
        assert!(scope.contains(&ProcessIdentity::from_row(&rows[1])));
        assert!(!scope.contains(&ProcessIdentity::from_row(&rows[0])));
    }

    #[test]
    fn invalid_edges_do_not_inherit() {
        let mut scope = TrackingScope::default();
        let mut unknown = row(7, Some(1), "unknown.exe", 7);
        unknown.start_time = None;
        let rows = [
            row(1, None, "root.exe", 10),
            row(2, Some(1), "older.exe", 2),
            row(3, Some(4), "root.exe", 1),
            row(4, Some(3), "cycle.exe", 2),
            row(5, None, "root.exe", 1),
            row(5, None, "duplicate.exe", 1),
            row(6, Some(5), "ambiguous.exe", 2),
            unknown,
        ];
        scope.update(&rows, &roots(&["root.exe"]));
        for index in [1, 3, 6, 7] {
            assert!(!scope.contains(&ProcessIdentity::from_row(&rows[index])));
        }
    }
    #[test]
    fn deep_unordered_tree_is_iterative() {
        let mut rows = (1..=4096)
            .map(|pid| {
                row(
                    pid,
                    (pid > 1).then_some(pid - 1),
                    if pid == 1 { "root.exe" } else { "worker.exe" },
                    pid as u64,
                )
            })
            .collect::<Vec<_>>();
        rows.reverse();
        let mut scope = TrackingScope::default();
        scope.update(&rows, &roots(&["root.exe"]));
        assert_eq!(scope.identities().len(), rows.len());
    }
}
