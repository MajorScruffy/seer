use crate::ir::{Outline, OutlineNode};

pub fn print(outline: &Outline) -> String {
    if outline.roots.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, root) in outline.roots.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        emit(root, 0, &mut out);
    }
    out
}

fn emit(node: &OutlineNode, depth: usize, out: &mut String) {
    for _ in 0..(2 * depth) {
        out.push(' ');
    }
    out.push_str(&node.text);
    out.push('\n');
    for child in &node.children {
        emit(child, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(text: &str, children: Vec<OutlineNode>) -> OutlineNode {
        OutlineNode {
            text: text.to_string(),
            children,
        }
    }

    #[test]
    fn print_empty() {
        let outline = Outline { roots: vec![] };
        assert_eq!(print(&outline), "");
    }

    #[test]
    fn print_two_roots() {
        let outline = Outline {
            roots: vec![
                node("fn a", vec![node("return", vec![])]),
                node("fn b", vec![node("return", vec![])]),
            ],
        };
        assert_eq!(print(&outline), "fn a\n  return\n\nfn b\n  return\n");
    }

    #[test]
    fn print_indent_and_no_trailing_ws() {
        let outline = Outline {
            roots: vec![node(
                "fn outer",
                vec![node("if cond", vec![node("return", vec![])])],
            )],
        };
        let out = print(&outline);
        assert_eq!(out, "fn outer\n  if cond\n    return\n");
        assert!(out.ends_with('\n'));
        for line in out.lines() {
            assert_eq!(line, line.trim_end());
            assert!(!line.ends_with(' '));
            assert!(!line.ends_with('\t'));
        }
    }
}
