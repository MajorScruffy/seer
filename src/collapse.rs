/// `literal_ranges` are half-open byte offsets into `src` that must be copied verbatim.
pub fn collapse(src: &str, literal_ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_ws_run = false;
    for (i, ch) in src.char_indices() {
        let in_literal = literal_ranges
            .iter()
            .any(|&(start, end)| i >= start && i < end);
        if in_literal {
            if in_ws_run {
                out.push(' ');
                in_ws_run = false;
            }
            out.push(ch);
        } else if ch.is_whitespace() {
            in_ws_run = true;
        } else {
            if in_ws_run {
                out.push(' ');
                in_ws_run = false;
            }
            out.push(ch);
        }
    }
    out.trim_matches(' ').to_string()
}

/// Drop a leading `::`, then at most one of `std::` / `core::` / `alloc::`.
pub fn strip_std(s: &str) -> String {
    let s = s.strip_prefix("::").unwrap_or(s);
    for prefix in ["std::", "core::", "alloc::"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_ws_runs() {
        assert_eq!(collapse("  foo   \n\t bar  ", &[]), "foo bar");
        assert_eq!(collapse("a\n\nb", &[]), "a b");
        assert_eq!(collapse("   ", &[]), "");
    }

    #[test]
    fn collapse_preserves_string_interior() {
        let src = r#"x("a  b")"#;
        let start = src.find('"').unwrap();
        let end = start + r#""a  b""#.len();
        assert_eq!(collapse(src, &[(start, end)]), r#"x("a  b")"#);
        assert_eq!(collapse(src, &[]), r#"x("a b")"#);
    }

    #[test]
    fn strip_std_std_fs_write() {
        assert_eq!(
            strip_std("std::fs::write(path, data)"),
            "fs::write(path, data)"
        );
    }

    #[test]
    fn strip_std_serde_unchanged() {
        assert_eq!(strip_std("serde_json::x"), "serde_json::x");
    }

    #[test]
    fn strip_std_core() {
        assert_eq!(strip_std("core::mem::drop(x)"), "mem::drop(x)");
    }

    #[test]
    fn strip_std_not_applied_in_this_unit() {
        assert_eq!(
            collapse("std::fs::write(path, data)", &[]),
            "std::fs::write(path, data)"
        );
    }
}
