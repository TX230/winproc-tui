use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathCompletion {
    None,
    Replaced {
        value: String,
        cursor: usize,
        match_count: usize,
        candidate_index: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PathCompletionState {
    session: Option<CompletionSession>,
}

impl PathCompletionState {
    pub(crate) fn reset(&mut self) {
        self.session = None;
    }

    pub(crate) fn complete_directory_path(&mut self, value: &str, cursor: usize) -> PathCompletion {
        let cursor = floor_char_boundary(value, cursor.min(value.len()));

        if let Some(session) = &mut self.session {
            if session.matches(value, cursor) {
                session.advance();
                return session.to_completion();
            }
        }

        let (head, tail) = value.split_at(cursor);
        let query = directory_query(head);
        let Ok(candidates) = directory_candidates(&query.parent, &query.prefix) else {
            self.reset();
            return PathCompletion::None;
        };

        if candidates.is_empty() {
            self.reset();
            return PathCompletion::None;
        }

        self.session = Some(CompletionSession {
            prefix: head[..query.replace_start].to_string(),
            tail: tail.to_string(),
            candidates,
            index: 0,
            append_separator: !tail.starts_with(['\\', '/']),
        });
        self.session
            .as_ref()
            .expect("completion session was just created")
            .to_completion()
    }
}

#[cfg(test)]
pub(crate) fn complete_directory_path(value: &str, cursor: usize) -> PathCompletion {
    PathCompletionState::default().complete_directory_path(value, cursor)
}

#[derive(Debug, Clone)]
struct CompletionSession {
    prefix: String,
    tail: String,
    candidates: Vec<String>,
    index: usize,
    append_separator: bool,
}

impl CompletionSession {
    fn matches(&self, value: &str, cursor: usize) -> bool {
        let (expected_value, expected_cursor) = self.value_and_cursor();
        value == expected_value && cursor == expected_cursor
    }

    fn advance(&mut self) {
        self.index = (self.index + 1) % self.candidates.len();
    }

    fn to_completion(&self) -> PathCompletion {
        let (value, cursor) = self.value_and_cursor();
        PathCompletion::Replaced {
            value,
            cursor,
            match_count: self.candidates.len(),
            candidate_index: self.index,
        }
    }

    fn value_and_cursor(&self) -> (String, usize) {
        let mut completed = self.prefix.clone();
        completed.push_str(&self.candidates[self.index]);
        if self.append_separator && !completed.ends_with(['\\', '/']) {
            completed.push(std::path::MAIN_SEPARATOR);
        }
        let cursor = completed.len();
        completed.push_str(&self.tail);
        (completed, cursor)
    }
}

#[derive(Debug, Clone)]
struct DirectoryQuery {
    parent: PathBuf,
    prefix: String,
    replace_start: usize,
}

fn directory_query(value: &str) -> DirectoryQuery {
    if value.is_empty() {
        return DirectoryQuery {
            parent: PathBuf::from("."),
            prefix: String::new(),
            replace_start: 0,
        };
    }

    if value.ends_with(['\\', '/']) {
        return DirectoryQuery {
            parent: PathBuf::from(value),
            prefix: String::new(),
            replace_start: value.len(),
        };
    }

    let separator = value.rfind(['\\', '/']);
    match separator {
        Some(index) => DirectoryQuery {
            parent: PathBuf::from(&value[..=index]),
            prefix: value[index + 1..].to_string(),
            replace_start: index + 1,
        },
        None => DirectoryQuery {
            parent: PathBuf::from("."),
            prefix: value.to_string(),
            replace_start: 0,
        },
    }
}

fn directory_candidates(parent: &Path, prefix: &str) -> std::io::Result<Vec<String>> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if starts_with_ignore_ascii_case(&name, prefix) {
            candidates.push(name);
        }
    }
    candidates.sort_by_key(|value| value.to_ascii_lowercase());
    Ok(candidates)
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.to_lowercase().starts_with(&prefix.to_lowercase())
}

fn floor_char_boundary(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "winproc-tui-path-completion-{label}-{}",
                std::process::id()
            ))
    }

    fn draft_for_child(root: &Path, prefix: &str) -> String {
        format!("{}{}{}", root.display(), std::path::MAIN_SEPARATOR, prefix)
    }

    #[test]
    fn directory_query_splits_parent_and_prefix() {
        let query = directory_query(r"C:\logs\alp");

        assert_eq!(query.parent, PathBuf::from(r"C:\logs\"));
        assert_eq!(query.prefix, "alp");
        assert_eq!(query.replace_start, r"C:\logs\".len());
    }

    #[test]
    fn completes_unique_directory_and_appends_separator() {
        let root = test_root("unique");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::write(root.join("alps.txt"), "not a directory").unwrap();
        let draft = draft_for_child(&root, "al");

        let result = complete_directory_path(&draft, draft.len());

        assert_eq!(
            result,
            PathCompletion::Replaced {
                value: draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR)),
                cursor: draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR))
                    .len(),
                match_count: 1,
                candidate_index: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completes_first_directory_when_multiple_match() {
        let root = test_root("first");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join("alpine")).unwrap();
        let draft = draft_for_child(&root, "al");
        let expected = draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR));

        let result = complete_directory_path(&draft, draft.len());

        assert_eq!(
            result,
            PathCompletion::Replaced {
                value: expected.clone(),
                cursor: expected.len(),
                match_count: 2,
                candidate_index: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tab_style_completion_cycles_multiple_directories() {
        let root = test_root("cycle");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join("alpine")).unwrap();
        let draft = draft_for_child(&root, "al");
        let first = draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR));
        let second = draft_for_child(&root, &format!("alpine{}", std::path::MAIN_SEPARATOR));
        let mut completion = PathCompletionState::default();

        let first_result = completion.complete_directory_path(&draft, draft.len());
        let second_result = match first_result {
            PathCompletion::Replaced { value, cursor, .. } => {
                completion.complete_directory_path(&value, cursor)
            }
            PathCompletion::None => panic!("expected first completion"),
        };

        assert_eq!(
            second_result,
            PathCompletion::Replaced {
                value: second.clone(),
                cursor: second.len(),
                match_count: 2,
                candidate_index: 1,
            }
        );
        let third_result = completion.complete_directory_path(&second, second.len());
        assert_eq!(
            third_result,
            PathCompletion::Replaced {
                value: first.clone(),
                cursor: first.len(),
                match_count: 2,
                candidate_index: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completes_first_directory_when_no_common_prefix_can_extend() {
        let root = test_root("multiple");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join("beta")).unwrap();
        let draft = draft_for_child(&root, "");
        let expected = draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR));

        let result = complete_directory_path(&draft, draft.len());

        assert_eq!(
            result,
            PathCompletion::Replaced {
                value: expected.clone(),
                cursor: expected.len(),
                match_count: 2,
                candidate_index: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completes_at_cursor_without_removing_tail() {
        let root = test_root("cursor");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        let head = draft_for_child(&root, "al");
        let draft = format!("{head}-suffix");

        let result = complete_directory_path(&draft, head.len());

        assert_eq!(
            result,
            PathCompletion::Replaced {
                value: format!(
                    "{}-suffix",
                    draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR))
                ),
                cursor: draft_for_child(&root, &format!("alpha{}", std::path::MAIN_SEPARATOR))
                    .len(),
                match_count: 1,
                candidate_index: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completes_directory_before_existing_separator_without_doubling_it() {
        let root = test_root("tail-separator");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        let head = draft_for_child(&root, "al");
        let draft = format!("{head}{}capture.log", std::path::MAIN_SEPARATOR);
        let expected = format!(
            "{}{}capture.log",
            draft_for_child(&root, "alpha"),
            std::path::MAIN_SEPARATOR
        );

        let result = complete_directory_path(&draft, head.len());

        assert_eq!(
            result,
            PathCompletion::Replaced {
                value: expected.clone(),
                cursor: draft_for_child(&root, "alpha").len(),
                match_count: 1,
                candidate_index: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }
}
