/// Identity of a `function_item` in the analyzed set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnId {
    /// POSIX relative path, or `<stdin>`.
    pub file: String,
    pub start_byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FnKind {
    Free,
    Method,
}

/// Indexed `function_item` or `function_signature_item`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnDef {
    pub id: FnId,
    pub name: String,
    pub kind: FnKind,
    pub module: Vec<String>,
    pub nested: bool,
    pub has_body: bool,
    pub body: Vec<RawNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    pub roots: Vec<OutlineNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineNode {
    pub text: String,
    pub children: Vec<OutlineNode>,
}

/// Unexpanded extraction node. Resolve keys are not printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawNode {
    Control {
        text: String,
        children: Vec<RawNode>,
    },
    NestedFn {
        name: String,
        children: Vec<RawNode>,
    },
    Call {
        site: CallSite,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    /// Already collapsed + std-prefix-stripped.
    pub display: String,
    pub kind: CallKind,
    pub is_macro: bool,
    /// Same string as `FnId.file` of the source that contains this call.
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallKind {
    /// `foo()`, `foo::bar()`, `crate::foo()`, `todo!()`
    Free { path: Vec<String> },
    /// `recv.method(...)` — name is the last segment
    Method { name: String },
}

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
    out.push_str(&" ".repeat(2 * depth));
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
