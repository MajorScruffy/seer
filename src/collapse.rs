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

/// Collect ranges of descendant nodes whose kind is exactly one of
/// `string_literal`, `raw_string_literal`, `char_literal`, then
/// `collapse(node.utf8_text(src), ranges_relative_to_that_slice)`.
pub fn collapse_node(node: tree_sitter::Node, src: &str) -> String {
    let text = node.utf8_text(src.as_bytes()).unwrap_or("");
    let ranges = literal_ranges(node);
    collapse(text, &ranges)
}

const LITERAL_KINDS: &[&str] = &["string_literal", "raw_string_literal", "char_literal"];

/// Half-open ranges relative to `node.start_byte()`.
pub(crate) fn literal_ranges(node: tree_sitter::Node) -> Vec<(usize, usize)> {
    let origin = node.start_byte();
    let mut ranges = Vec::new();
    collect_literal_ranges(node, origin, &mut ranges);
    ranges
}

fn collect_literal_ranges(node: tree_sitter::Node, origin: usize, out: &mut Vec<(usize, usize)>) {
    if LITERAL_KINDS.contains(&node.kind()) {
        out.push((node.start_byte() - origin, node.end_byte() - origin));
        return;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_literal_ranges(child, origin, out);
        }
    }
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

    #[test]
    fn collapse_node_only_three_literal_kinds() {
        use crate::parse::{find_kind, parse_rust};

        let src = r#"fn f() { foo(  "a  b"  ,  r"c  d"  ,  ' '  ,  /*  not  lit  */  1  ); }"#;
        let tree = parse_rust(src);
        let call = find_kind(tree.root_node(), "call_expression").expect("call");
        assert_eq!(
            collapse_node(call, src),
            r#"foo( "a  b" , r"c  d" , ' ' , /* not lit */ 1 )"#
        );

        let origin = call.start_byte();
        let ranges = literal_ranges(call);
        let mut kinds = Vec::new();
        collect_kinds_at_ranges(call, origin, &ranges, &mut kinds);
        for kind in &kinds {
            assert!(
                LITERAL_KINDS.contains(kind),
                "literal range came from {kind}, not one of the three kinds"
            );
        }
        assert!(kinds.contains(&"string_literal"));
        assert!(kinds.contains(&"raw_string_literal"));
        assert!(kinds.contains(&"char_literal"));
    }

    fn collect_kinds_at_ranges<'a>(
        node: tree_sitter::Node<'a>,
        origin: usize,
        ranges: &[(usize, usize)],
        out: &mut Vec<&'a str>,
    ) {
        let rel = (node.start_byte() - origin, node.end_byte() - origin);
        if ranges.contains(&rel) {
            out.push(node.kind());
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                collect_kinds_at_ranges(child, origin, ranges, out);
            }
        }
    }
}
