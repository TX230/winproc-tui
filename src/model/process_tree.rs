use std::collections::{HashMap, HashSet};

use crate::model::{ProcessIdentity, ProcessRow, SortSpec, compare_process_rows};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessTreeRow {
    pub(crate) row_index: usize,
    pub(crate) depth: usize,
    pub(crate) has_children: bool,
    pub(crate) expanded: bool,
    pub(crate) context_only: bool,
}

#[derive(Debug, Clone)]
struct ProcessTreeNode {
    row_index: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessForest {
    nodes: Vec<ProcessTreeNode>,
    roots: Vec<usize>,
    row_to_node: HashMap<usize, usize>,
}

impl ProcessForest {
    pub(crate) fn build(
        rows: &[ProcessRow],
        candidate_rows: impl IntoIterator<Item = usize>,
        sort: Option<SortSpec>,
    ) -> Self {
        let mut seen_rows = HashSet::new();
        let mut nodes = candidate_rows
            .into_iter()
            .filter(|row_index| *row_index < rows.len() && seen_rows.insert(*row_index))
            .map(|row_index| ProcessTreeNode {
                row_index,
                parent: None,
                children: Vec::new(),
            })
            .collect::<Vec<_>>();
        let row_to_node = nodes
            .iter()
            .enumerate()
            .map(|(node_index, node)| (node.row_index, node_index))
            .collect::<HashMap<_, _>>();

        let mut pid_to_node = HashMap::<u32, Option<usize>>::new();
        for (node_index, node) in nodes.iter().enumerate() {
            let pid = rows[node.row_index].pid;
            pid_to_node
                .entry(pid)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(node_index));
        }

        for (node_index, node) in nodes.iter_mut().enumerate() {
            node.parent = rows[node.row_index]
                .parent_pid
                .and_then(|parent_pid| pid_to_node.get(&parent_pid).copied().flatten())
                .filter(|parent_index| *parent_index != node_index);
        }

        break_parent_cycles(&mut nodes);

        let mut roots = Vec::new();
        for node_index in 0..nodes.len() {
            if let Some(parent_index) = nodes[node_index].parent {
                nodes[parent_index].children.push(node_index);
            } else {
                roots.push(node_index);
            }
        }

        sort_node_group(&mut roots, &nodes, rows, sort);
        for node_index in 0..nodes.len() {
            let mut children = std::mem::take(&mut nodes[node_index].children);
            sort_node_group(&mut children, &nodes, rows, sort);
            nodes[node_index].children = children;
        }

        Self {
            nodes,
            roots,
            row_to_node,
        }
    }

    pub(crate) fn visible_rows(
        &self,
        rows: &[ProcessRow],
        matches: Option<&HashSet<usize>>,
        collapsed: &HashSet<ProcessIdentity>,
    ) -> Vec<ProcessTreeRow> {
        let mut included = vec![matches.is_none(); self.nodes.len()];
        if let Some(matches) = matches {
            for row_index in matches {
                let Some(mut node_index) = self.row_to_node.get(row_index).copied() else {
                    continue;
                };
                loop {
                    if included[node_index] {
                        break;
                    }
                    included[node_index] = true;
                    let Some(parent_index) = self.nodes[node_index].parent else {
                        break;
                    };
                    node_index = parent_index;
                }
            }
        }

        let force_matching_paths_open = matches.is_some();
        let mut flattened = Vec::new();
        let mut stack = self
            .roots
            .iter()
            .rev()
            .filter(|node_index| included[**node_index])
            .map(|node_index| (*node_index, 0usize))
            .collect::<Vec<_>>();
        while let Some((node_index, depth)) = stack.pop() {
            let node = &self.nodes[node_index];
            let has_children = node
                .children
                .iter()
                .any(|child_index| included[*child_index]);
            let identity = ProcessIdentity::from_row(&rows[node.row_index]);
            let expanded =
                has_children && (force_matching_paths_open || !collapsed.contains(&identity));
            flattened.push(ProcessTreeRow {
                row_index: node.row_index,
                depth,
                has_children,
                expanded,
                context_only: matches.is_some_and(|matches| !matches.contains(&node.row_index)),
            });
            if expanded {
                for child_index in node.children.iter().rev() {
                    if included[*child_index] {
                        stack.push((*child_index, depth.saturating_add(1)));
                    }
                }
            }
        }
        flattened
    }

    pub(crate) fn parent_row_index(&self, row_index: usize) -> Option<usize> {
        let node_index = self.row_to_node.get(&row_index).copied()?;
        self.nodes[node_index]
            .parent
            .map(|parent_index| self.nodes[parent_index].row_index)
    }
}

fn break_parent_cycles(nodes: &mut [ProcessTreeNode]) {
    let mut state = vec![0u8; nodes.len()];
    for start in 0..nodes.len() {
        if state[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(node_index) = current {
            match state[node_index] {
                0 => {
                    state[node_index] = 1;
                    path.push(node_index);
                    current = nodes[node_index].parent;
                }
                1 => {
                    if let Some(cycle_start) = path.iter().position(|item| *item == node_index) {
                        for cycle_node in &path[cycle_start..] {
                            nodes[*cycle_node].parent = None;
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
        for node_index in path {
            state[node_index] = 2;
        }
    }
}

fn sort_node_group(
    group: &mut [usize],
    nodes: &[ProcessTreeNode],
    rows: &[ProcessRow],
    sort: Option<SortSpec>,
) {
    group.sort_by(|left, right| {
        let left_row = nodes[*left].row_index;
        let right_row = nodes[*right].row_index;
        sort.map_or_else(
            || left.cmp(right),
            |sort| {
                compare_process_rows(&rows[left_row], &rows[right_row], sort)
                    .then_with(|| left_row.cmp(&right_row))
            },
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MetricColumn, SortColumn, SortDirection};

    fn row(pid: u32, parent_pid: Option<u32>, name: &str) -> ProcessRow {
        ProcessRow {
            pid,
            parent_pid,
            name: name.to_string(),
            start_time: Some(1_700_000_000 + u64::from(pid)),
            ..ProcessRow::default()
        }
    }

    fn visible_indices(forest: &ProcessForest, rows: &[ProcessRow]) -> Vec<usize> {
        forest
            .visible_rows(rows, None, &HashSet::new())
            .into_iter()
            .map(|row| row.row_index)
            .collect()
    }

    #[test]
    fn parent_child_and_grandchild_form_a_preorder_forest() {
        let rows = vec![
            row(30, Some(20), "grandchild"),
            row(10, None, "parent"),
            row(20, Some(10), "child"),
            row(40, None, "other-root"),
        ];
        let forest = ProcessForest::build(
            &rows,
            0..rows.len(),
            Some(SortSpec {
                column: SortColumn::Pid,
                direction: SortDirection::Asc,
            }),
        );
        let visible = forest.visible_rows(&rows, None, &HashSet::new());

        assert_eq!(
            visible
                .iter()
                .map(|item| (rows[item.row_index].pid, item.depth))
                .collect::<Vec<_>>(),
            vec![(10, 0), (20, 1), (30, 2), (40, 0)]
        );
        assert_eq!(forest.parent_row_index(0), Some(2));
        assert_eq!(forest.parent_row_index(2), Some(1));
    }

    #[test]
    fn missing_self_duplicate_and_cyclic_parents_become_roots() {
        let rows = vec![
            row(1, Some(2), "cycle-a"),
            row(2, Some(1), "cycle-b"),
            row(3, Some(3), "self"),
            row(4, Some(999), "missing"),
            row(5, None, "duplicate-a"),
            row(5, None, "duplicate-b"),
            row(6, Some(5), "ambiguous-parent"),
            row(7, Some(1), "cycle-child"),
        ];
        let forest = ProcessForest::build(&rows, 0..rows.len(), None);

        for index in 0..7 {
            assert_eq!(forest.parent_row_index(index), None, "row {index}");
        }
        assert_eq!(forest.parent_row_index(7), Some(0));
        assert_eq!(visible_indices(&forest, &rows).len(), rows.len());
    }

    #[test]
    fn deep_input_is_flattened_iteratively() {
        let rows = (0..10_000u32)
            .map(|pid| row(pid + 1, (pid > 0).then_some(pid), "deep"))
            .collect::<Vec<_>>();
        let forest = ProcessForest::build(&rows, 0..rows.len(), None);
        let visible = forest.visible_rows(&rows, None, &HashSet::new());

        assert_eq!(visible.len(), rows.len());
        assert_eq!(visible.last().map(|item| item.depth), Some(9_999));
    }

    #[test]
    fn filtering_includes_ancestors_as_context_without_unrelated_rows() {
        let rows = vec![
            row(1, None, "root"),
            row(2, Some(1), "child"),
            row(3, Some(2), "match"),
            row(4, None, "unrelated"),
        ];
        let forest = ProcessForest::build(&rows, 0..rows.len(), None);
        let matches = HashSet::from([2usize]);
        let visible = forest.visible_rows(&rows, Some(&matches), &HashSet::new());

        assert_eq!(
            visible
                .iter()
                .map(|item| (item.row_index, item.context_only))
                .collect::<Vec<_>>(),
            vec![(0, true), (1, true), (2, false)]
        );
    }

    #[test]
    fn collapsed_identity_hides_descendants_but_filter_paths_remain_visible() {
        let rows = vec![row(1, None, "root"), row(2, Some(1), "match")];
        let forest = ProcessForest::build(&rows, 0..rows.len(), None);
        let collapsed = HashSet::from([ProcessIdentity::from_row(&rows[0])]);

        let collapsed_rows = forest.visible_rows(&rows, None, &collapsed);
        assert_eq!(collapsed_rows.len(), 1);
        assert!(!collapsed_rows[0].expanded);

        let matches = HashSet::from([1usize]);
        let filtered_rows = forest.visible_rows(&rows, Some(&matches), &collapsed);
        assert_eq!(filtered_rows.len(), 2);
        assert!(filtered_rows[0].expanded);
    }

    #[test]
    fn pid_reuse_is_resolved_only_within_each_snapshot() {
        let first = vec![row(10, None, "old-parent"), row(20, Some(10), "child")];
        let first_forest = ProcessForest::build(&first, 0..first.len(), None);
        assert_eq!(first_forest.parent_row_index(1), Some(0));

        let mut reused_parent = row(10, None, "new-parent");
        reused_parent.start_time = Some(1_800_000_000);
        let second = vec![reused_parent];
        let second_forest = ProcessForest::build(&second, 0..second.len(), None);
        assert_eq!(second_forest.parent_row_index(0), None);
        assert_eq!(visible_indices(&second_forest, &second), vec![0]);
    }

    #[test]
    fn roots_and_siblings_follow_every_sort_column_in_both_directions() {
        let columns = std::iter::once(SortColumn::Pid)
            .chain(std::iter::once(SortColumn::ProcessName))
            .chain(MetricColumn::ALL.into_iter().map(SortColumn::Metric));
        for column in columns {
            for direction in [SortDirection::Asc, SortDirection::Desc] {
                let mut roots = vec![row(10, None, "low"), row(20, None, "high")];
                set_sort_value(&mut roots[0], column, false);
                set_sort_value(&mut roots[1], column, true);
                let forest = ProcessForest::build(
                    &roots,
                    0..roots.len(),
                    Some(SortSpec { column, direction }),
                );
                let expected = if direction == SortDirection::Asc {
                    vec![0, 1]
                } else {
                    vec![1, 0]
                };
                assert_eq!(
                    visible_indices(&forest, &roots),
                    expected,
                    "roots {column:?}"
                );

                let mut siblings = vec![
                    row(1, None, "parent"),
                    row(10, Some(1), "low"),
                    row(20, Some(1), "high"),
                ];
                set_sort_value(&mut siblings[1], column, false);
                set_sort_value(&mut siblings[2], column, true);
                let forest = ProcessForest::build(
                    &siblings,
                    0..siblings.len(),
                    Some(SortSpec { column, direction }),
                );
                let expected = if direction == SortDirection::Asc {
                    vec![0, 1, 2]
                } else {
                    vec![0, 2, 1]
                };
                assert_eq!(
                    visible_indices(&forest, &siblings),
                    expected,
                    "siblings {column:?}"
                );
            }
        }
    }

    fn set_sort_value(row: &mut ProcessRow, column: SortColumn, high: bool) {
        let integer = if high { 20 } else { 10 };
        let float = if high { 20.0 } else { 10.0 };
        match column {
            SortColumn::Pid => row.pid = integer as u32,
            SortColumn::ProcessName => row.name = if high { "z" } else { "a" }.to_string(),
            SortColumn::Metric(MetricColumn::CpuPercent) => row.cpu_percent = Some(float),
            SortColumn::Metric(MetricColumn::PrivateBytes) => row.private_bytes = Some(integer),
            SortColumn::Metric(MetricColumn::WorksetBytes) => row.workset_bytes = Some(integer),
            SortColumn::Metric(MetricColumn::WorksetPrivateBytes) => {
                row.workset_private_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::WorksetShareableBytes) => {
                row.workset_shareable_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::ThreadCount) => row.thread_count = Some(integer),
            SortColumn::Metric(MetricColumn::HandleCount) => row.handle_count = Some(integer),
            SortColumn::Metric(MetricColumn::UserObjectCount) => {
                row.user_object_count = Some(integer)
            }
            SortColumn::Metric(MetricColumn::GdiObjectCount) => {
                row.gdi_object_count = Some(integer)
            }
            SortColumn::Metric(MetricColumn::GpuPercent) => row.gpu_percent = Some(float),
            SortColumn::Metric(MetricColumn::DotNetHeapBytes) => {
                row.dotnet_heap_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcGen0HeapBytes) => {
                row.dotnet_gc_gen0_heap_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcGen1HeapBytes) => {
                row.dotnet_gc_gen1_heap_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcGen2HeapBytes) => {
                row.dotnet_gc_gen2_heap_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcLohBytes) => {
                row.dotnet_gc_loh_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcPohBytes) => {
                row.dotnet_gc_poh_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcCommittedBytes) => {
                row.dotnet_gc_committed_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetGcFragmentationBytes) => {
                row.dotnet_gc_fragmentation_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::DotNetAllocationBytesPerSec) => {
                row.dotnet_allocation_bytes_per_sec = Some(integer)
            }
            SortColumn::Metric(MetricColumn::GpuDedicatedBytes) => {
                row.gpu_dedicated_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::GpuSharedBytes) => {
                row.gpu_shared_bytes = Some(integer)
            }
            SortColumn::Metric(MetricColumn::IoReadBytesPerSec) => {
                row.io_read_bytes_per_sec = Some(integer)
            }
            SortColumn::Metric(MetricColumn::IoWriteBytesPerSec) => {
                row.io_write_bytes_per_sec = Some(integer)
            }
            SortColumn::Metric(MetricColumn::FullPath) => {
                row.executable_path = Some(if high { "z" } else { "a" }.to_string())
            }
        }
    }
}
