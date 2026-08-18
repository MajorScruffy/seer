use tree_sitter::Node;

use crate::collapse::collapse_node;
use crate::ir::{CallKind, FnKind};

pub const FUNCTION_ITEM: &str = "function_item";
pub const IF_EXPRESSION: &str = "if_expression";
pub const FOR_EXPRESSION: &str = "for_expression";
pub const WHILE_EXPRESSION: &str = "while_expression";
pub const LOOP_EXPRESSION: &str = "loop_expression";
pub const MATCH_EXPRESSION: &str = "match_expression";
pub const MATCH_ARM: &str = "match_arm";
pub const RETURN_EXPRESSION: &str = "return_expression";
pub const BREAK_EXPRESSION: &str = "break_expression";
pub const CONTINUE_EXPRESSION: &str = "continue_expression";
pub const TRY_BLOCK: &str = "try_block";
pub const CALL_EXPRESSION: &str = "call_expression";
pub const MACRO_INVOCATION: &str = "macro_invocation";
pub const LET_DECLARATION: &str = "let_declaration";
pub const EXPRESSION_STATEMENT: &str = "expression_statement";
pub const CLOSURE_EXPRESSION: &str = "closure_expression";
pub const ASSIGNMENT_EXPRESSION: &str = "assignment_expression";
pub const COMPOUND_ASSIGNMENT: &str = "compound_assignment_expr";
pub const ERROR: &str = "ERROR";
pub const LABEL: &str = "label";
pub const BLOCK: &str = "block";
pub const FIELD_EXPRESSION: &str = "field_expression";
pub const GENERIC_FUNCTION: &str = "generic_function";
pub const IMPL_ITEM: &str = "impl_item";
pub const TRAIT_ITEM: &str = "trait_item";
pub const SOURCE_FILE: &str = "source_file";
pub const MOD_ITEM: &str = "mod_item";

pub fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

pub fn first_named_child(node: Node) -> Option<Node> {
    node.named_child(0)
}

/// First named child whose kind is one of `kinds` (skips extras such as comments).
pub fn first_named_child_kind<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if kinds.iter().any(|k| child.kind() == *k) {
                return Some(child);
            }
        }
    }
    None
}

pub fn fn_name(node: Node, src: &str) -> String {
    node.child_by_field_name("name")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_default()
}

/// Nearest of `{impl_item, trait_item, source_file, mod_item}` decides kind.
pub fn classify_function_item(node: Node) -> FnKind {
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            IMPL_ITEM | TRAIT_ITEM => return FnKind::Method,
            SOURCE_FILE | MOD_ITEM => return FnKind::Free,
            _ => cur = n.parent(),
        }
    }
    FnKind::Free
}

pub fn label_prefix(node: Node, src: &str) -> String {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if child.kind() == LABEL {
                return format!("{}: ", collapse_node(child, src));
            }
        }
    }
    String::new()
}

pub fn header_if(keyword: &str, cond: Node, src: &str) -> String {
    format!("{keyword} {}", collapse_node(cond, src))
}

pub fn header_for(node: Node, src: &str) -> String {
    let pattern = node
        .child_by_field_name("pattern")
        .map(|n| collapse_node(n, src))
        .unwrap_or_default();
    let value = node
        .child_by_field_name("value")
        .map(|n| collapse_node(n, src))
        .unwrap_or_default();
    format!("{}for {pattern} in {value}", label_prefix(node, src))
}

pub fn header_while(node: Node, src: &str) -> String {
    let cond = node
        .child_by_field_name("condition")
        .map(|n| collapse_node(n, src))
        .unwrap_or_default();
    format!("{}while {cond}", label_prefix(node, src))
}

pub fn header_loop(node: Node, src: &str) -> String {
    format!("{}loop", label_prefix(node, src))
}

pub fn header_match(node: Node, src: &str) -> String {
    let value = node
        .child_by_field_name("value")
        .map(|n| collapse_node(n, src))
        .unwrap_or_default();
    format!("match {value}")
}

pub fn path_segments(node: Node, src: &str) -> Vec<String> {
    match node.kind() {
        "identifier" | "crate" | "self" | "super" | "metavariable" | "field_identifier"
        | "type_identifier" => vec![node_text(node, src).to_string()],
        "scoped_identifier" => {
            let mut segs = node
                .child_by_field_name("path")
                .map(|p| path_segments(p, src))
                .unwrap_or_default();
            if let Some(name) = node.child_by_field_name("name") {
                segs.extend(path_segments(name, src));
            }
            segs
        }
        GENERIC_FUNCTION => node
            .child_by_field_name("function")
            .map(|f| path_segments(f, src))
            .unwrap_or_default(),
        "generic_type" => node
            .child_by_field_name("type")
            .map(|t| path_segments(t, src))
            .unwrap_or_default(),
        _ => node
            .child_by_field_name("name")
            .map(|n| path_segments(n, src))
            .unwrap_or_default(),
    }
}

/// Method iff `function` is a `field_expression` (or generic of one).
pub fn call_kind(function_node: Node, src: &str) -> CallKind {
    let callee = if function_node.kind() == GENERIC_FUNCTION {
        function_node
            .child_by_field_name("function")
            .unwrap_or(function_node)
    } else {
        function_node
    };
    if callee.kind() == FIELD_EXPRESSION {
        let name = callee
            .child_by_field_name("field")
            .map(|f| node_text(f, src).to_string())
            .unwrap_or_default();
        CallKind::Method { name }
    } else {
        CallKind::Free {
            path: path_segments(callee, src),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_rust;

    fn find_fn<'a>(root: Node<'a>, src: &str, name: &str) -> Option<Node<'a>> {
        if root.kind() == FUNCTION_ITEM && fn_name(root, src) == name {
            return Some(root);
        }
        for i in 0..root.named_child_count() {
            if let Some(child) = root.named_child(i) {
                if let Some(found) = find_fn(child, src, name) {
                    return Some(found);
                }
            }
        }
        None
    }

    #[test]
    fn classify_method_vs_free() {
        let src = r#"
fn free_fn() {}
mod m {
    fn in_mod() {}
}
impl Foo {
    fn method() {}
}
fn outer() {
    fn inner() {}
}
"#;
        let tree = parse_rust(src);
        let root = tree.root_node();
        assert_eq!(
            classify_function_item(find_fn(root, src, "free_fn").unwrap()),
            FnKind::Free
        );
        assert_eq!(
            classify_function_item(find_fn(root, src, "in_mod").unwrap()),
            FnKind::Free
        );
        assert_eq!(
            classify_function_item(find_fn(root, src, "method").unwrap()),
            FnKind::Method
        );
        assert_eq!(
            classify_function_item(find_fn(root, src, "inner").unwrap()),
            FnKind::Free
        );
    }
}
