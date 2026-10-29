/// Case-insensitive AND terms. Quotes group literal whitespace; backslashes stay literal.
/// An unfinished quote groups through the end so partially typed queries remain usable.
pub(crate) fn terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut term = String::new();
    let mut quoted = false;
    for ch in query.chars() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch.is_whitespace() && !quoted {
            if !term.is_empty() {
                terms.push(std::mem::take(&mut term).to_lowercase());
            }
        } else {
            term.push(ch);
        }
    }
    if !term.is_empty() {
        terms.push(term.to_lowercase());
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn and_terms_preserve_quoted_spaces_unicode_and_windows_separators() {
        assert_eq!(
            terms(r#"  JAVA   "Program Files"  日本語 C:\work "#),
            vec!["java", "program files", "日本語", r"c:\work"]
        );
        assert_eq!(terms(" \t \n "), Vec::<String>::new());
        assert_eq!(terms(r#""unfinished phrase"#), vec!["unfinished phrase"]);
        assert_eq!(terms(r#""""#), Vec::<String>::new());
    }
}
