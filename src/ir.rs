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
    /// 1-based line of `id.start_byte`. Tree jumps need it; print decides the prefix.
    pub start_line: usize,
    pub name: String,
    pub kind: FnKind,
    pub module: Vec<String>,
    pub nested: bool,
    pub has_body: bool,
    /// Exclusive UTF-8 end offset of the definition node.
    pub end_byte: usize,
    pub body: Vec<RawNode>,
}

/// 1-based line of a byte offset.
pub fn line_of(src: &str, start_byte: usize) -> usize {
    let n = start_byte.min(src.len());
    src.as_bytes()[..n].iter().filter(|&&b| b == b'\n').count() + 1
}

impl FnDef {
    pub fn source_location(&self) -> Loc {
        Loc {
            file: self.id.file.clone(),
            line: self.start_line,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    pub roots: Vec<OutlineNode>,
}

/// File and 1-based line of a def or resolved call. Print prefixes this; expand does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loc {
    pub file: String,
    pub line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    /// `path:line` — tree jump targets.
    Jump,
    /// `path` only — flow/diff stays quiet on line shifts.
    Flow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineNode {
    pub text: String,
    pub loc: Option<Loc>,
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

pub fn print_outline(outline: &Outline, label: Label) -> String {
    if outline.roots.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, root) in outline.roots.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        write_outline_node(root, 0, label, &mut out);
    }
    out
}

fn write_outline_node(node: &OutlineNode, depth: usize, label: Label, out: &mut String) {
    out.push_str(&" ".repeat(2 * depth));
    match &node.loc {
        None => out.push_str(&node.text),
        Some(loc) => {
            out.push_str(&loc.file);
            if matches!(label, Label::Jump) {
                out.push(':');
                out.push_str(&loc.line.to_string());
            }
            out.push(' ');
            out.push_str(&node.text);
        }
    }
    out.push('\n');
    for child in &node.children {
        write_outline_node(child, depth + 1, label, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(text: &str, children: Vec<OutlineNode>) -> OutlineNode {
        OutlineNode {
            text: text.to_string(),
            loc: None,
            children,
        }
    }

    #[test]
    fn line_of_counts_newlines() {
        assert_eq!(line_of("fn a", 0), 1);
        assert_eq!(line_of("fn a\nfn b\n", 5), 2);
    }

    #[test]
    fn print_jump_and_flow_labels() {
        let outline = Outline {
            roots: vec![OutlineNode {
                text: "fn process".into(),
                loc: Some(Loc {
                    file: "a.rs".into(),
                    line: 1,
                }),
                children: vec![OutlineNode {
                    text: "handle()".into(),
                    loc: Some(Loc {
                        file: "a.rs".into(),
                        line: 13,
                    }),
                    children: vec![],
                }],
            }],
        };
        assert_eq!(
            print_outline(&outline, Label::Jump),
            "a.rs:1 fn process\n  a.rs:13 handle()\n"
        );
        assert_eq!(
            print_outline(&outline, Label::Flow),
            "a.rs fn process\n  a.rs handle()\n"
        );
    }

    #[test]
    fn print_empty() {
        let outline = Outline { roots: vec![] };
        assert_eq!(print_outline(&outline, Label::Jump), "");
    }

    #[test]
    fn print_two_roots() {
        let outline = Outline {
            roots: vec![
                node("fn a", vec![node("return", vec![])]),
                node("fn b", vec![node("return", vec![])]),
            ],
        };
        assert_eq!(
            print_outline(&outline, Label::Jump),
            "fn a\n  return\n\nfn b\n  return\n"
        );
    }

    #[test]
    fn print_indent_and_no_trailing_ws() {
        let outline = Outline {
            roots: vec![node(
                "fn outer",
                vec![node("if cond", vec![node("return", vec![])])],
            )],
        };
        let out = print_outline(&outline, Label::Jump);
        assert_eq!(out, "fn outer\n  if cond\n    return\n");
        assert!(out.ends_with('\n'));
        for line in out.lines() {
            assert_eq!(line, line.trim_end());
            assert!(!line.ends_with(' '));
            assert!(!line.ends_with('\t'));
        }
    }
}
