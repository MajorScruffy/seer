use tree_sitter::Node;

use crate::collapse::{collapse_node, strip_std};
use crate::ir::{CallSite, Outline, OutlineNode, RawNode};
use crate::lang::rust::{
    self, call_kind, first_named_child, first_named_child_kind, fn_name, header_for, header_if,
    header_loop, header_match, header_while, path_segments,
};
use crate::omit::{should_omit, UseMap};
use crate::print;

struct Ctx<'a> {
    src: &'a str,
    file: &'a str,
    uses: &'a UseMap,
}

/// Unexpanded body of one `function_item`. Does not emit a `fn` line for `fn_item` itself.
pub fn extract_fn(fn_item: Node, src: &str, file: &str, uses: &UseMap) -> Vec<RawNode> {
    let ctx = Ctx { src, file, uses };
    fn_item
        .child_by_field_name("body")
        .map(|body| walk(body, &ctx))
        .unwrap_or_default()
}

/// Non-nested body-bearing `function_item`s (not trait signatures), source order.
pub fn root_function_items<'a>(root: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    walk_root_fns(root, false, &mut out);
    out
}

fn walk_root_fns<'a>(node: Node<'a>, in_fn: bool, out: &mut Vec<Node<'a>>) {
    if node.kind() == rust::FUNCTION_ITEM {
        if !in_fn && node.child_by_field_name("body").is_some() {
            out.push(node);
        }
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                walk_root_fns(child, true, out);
            }
        }
        return;
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            walk_root_fns(child, in_fn, out);
        }
    }
}

/// First `function_item` whose name field equals `name`.
pub fn find_function_item<'a>(node: Node<'a>, src: &str, name: &str) -> Option<Node<'a>> {
    if node.kind() == rust::FUNCTION_ITEM && fn_name(node, src) == name {
        return Some(node);
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if let Some(found) = find_function_item(child, src, name) {
                return Some(found);
            }
        }
    }
    None
}

/// Print one function’s raw tree as an Outline (calls as leaves).
pub fn print_raw_fn(name: &str, body: &[RawNode]) -> String {
    print::print(&Outline {
        roots: vec![OutlineNode {
            text: format!("fn {name}"),
            children: raw_to_outline(body),
        }],
    })
}

fn raw_to_outline(nodes: &[RawNode]) -> Vec<OutlineNode> {
    nodes
        .iter()
        .map(|n| match n {
            RawNode::Control { text, children } => OutlineNode {
                text: text.clone(),
                children: raw_to_outline(children),
            },
            RawNode::NestedFn { name, children } => OutlineNode {
                text: format!("fn {name}"),
                children: raw_to_outline(children),
            },
            RawNode::Call { site } => OutlineNode {
                text: site.display.clone(),
                children: Vec::new(),
            },
        })
        .collect()
}

fn walk(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    match node.kind() {
        rust::ERROR => Vec::new(),
        rust::IF_EXPRESSION => extract_if(node, ctx),
        rust::FOR_EXPRESSION => vec![RawNode::Control {
            text: header_for(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        rust::WHILE_EXPRESSION => vec![RawNode::Control {
            text: header_while(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        rust::LOOP_EXPRESSION => vec![RawNode::Control {
            text: header_loop(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        rust::MATCH_EXPRESSION => vec![RawNode::Control {
            text: header_match(node, ctx.src),
            children: walk_match_arms(node, ctx),
        }],
        rust::MATCH_ARM => extract_match_arm(node, ctx),
        rust::RETURN_EXPRESSION | rust::BREAK_EXPRESSION | rust::CONTINUE_EXPRESSION => {
            vec![RawNode::Control {
                text: collapse_node(node, ctx.src),
                children: Vec::new(),
            }]
        }
        rust::TRY_BLOCK => vec![RawNode::Control {
            text: "try".to_string(),
            children: first_named_child_kind(node, &[rust::BLOCK])
                .map(|b| walk(b, ctx))
                .unwrap_or_default(),
        }],
        rust::CALL_EXPRESSION => emit_call(node, ctx, false),
        rust::MACRO_INVOCATION => emit_macro(node, ctx),
        rust::LET_DECLARATION => {
            let mut out = walk_field(node, "value", ctx);
            out.extend(walk_field(node, "alternative", ctx));
            out
        }
        rust::EXPRESSION_STATEMENT => first_named_child(node)
            .map(|e| walk(e, ctx))
            .unwrap_or_default(),
        "block" | "unsafe_block" | "async_block" | "const_block" | "gen_block" => {
            walk_named(node, ctx)
        }
        rust::CLOSURE_EXPRESSION => walk_field(node, "body", ctx),
        rust::ASSIGNMENT_EXPRESSION | rust::COMPOUND_ASSIGNMENT => walk_field(node, "right", ctx),
        rust::FUNCTION_ITEM => vec![RawNode::NestedFn {
            name: fn_name(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        kind if is_ignored_item(kind) => Vec::new(),
        _ => walk_named(node, ctx),
    }
}

fn is_ignored_item(kind: &str) -> bool {
    matches!(
        kind,
        "struct_item"
            | "enum_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "trait_item"
            | "impl_item"
            | "use_declaration"
            | "mod_item"
            | "attribute_item"
            | "inner_attribute_item"
            | "type_parameters"
            | "line_comment"
            | "block_comment"
    )
}

fn walk_named(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let mut out = Vec::new();
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            out.extend(walk(child, ctx));
        }
    }
    out
}

fn walk_field(node: Node, field: &str, ctx: &Ctx) -> Vec<RawNode> {
    node.child_by_field_name(field)
        .map(|n| walk(n, ctx))
        .unwrap_or_default()
}

/// Flat sibling chain: `if` / `else if` / `else` (not nested else→if).
fn extract_if(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let mut out = Vec::new();
    let mut keyword = "if";
    let mut current = node;
    loop {
        let text = current
            .child_by_field_name("condition")
            .map(|c| header_if(keyword, c, ctx.src))
            .unwrap_or_else(|| keyword.to_string());
        let children = walk_field(current, "consequence", ctx);
        out.push(RawNode::Control { text, children });
        let Some(alt) = current.child_by_field_name("alternative") else {
            break;
        };
        let Some(child) = first_named_child_kind(alt, &[rust::IF_EXPRESSION, rust::BLOCK]) else {
            break;
        };
        if child.kind() == rust::IF_EXPRESSION {
            keyword = "else if";
            current = child;
            continue;
        }
        out.push(RawNode::Control {
            text: "else".to_string(),
            children: walk(child, ctx),
        });
        break;
    }
    out
}

fn walk_match_arms(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..body.named_child_count() {
        if let Some(arm) = body.named_child(i) {
            if arm.kind() == rust::MATCH_ARM {
                out.extend(walk(arm, ctx));
            }
        }
    }
    out
}

fn extract_match_arm(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let text = node
        .child_by_field_name("pattern")
        .map(|p| collapse_node(p, ctx.src))
        .unwrap_or_default();
    let children = walk_field(node, "value", ctx);
    vec![RawNode::Control { text, children }]
}

fn emit_call(node: Node, ctx: &Ctx, is_macro: bool) -> Vec<RawNode> {
    let kind = if is_macro {
        crate::ir::CallKind::Free {
            path: node
                .child_by_field_name("macro")
                .map(|m| path_segments(m, ctx.src))
                .unwrap_or_default(),
        }
    } else {
        node.child_by_field_name("function")
            .map(|f| call_kind(f, ctx.src))
            .unwrap_or(crate::ir::CallKind::Free { path: Vec::new() })
    };
    let site = CallSite {
        display: strip_std(&collapse_node(node, ctx.src)),
        kind,
        is_macro,
        file: ctx.file.to_string(),
    };
    if should_omit(&site, ctx.uses) {
        Vec::new()
    } else {
        vec![RawNode::Call { site }]
    }
}

fn emit_macro(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    emit_call(node, ctx, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_rust;

    fn outline_of(src: &str, name: &str) -> String {
        let tree = parse_rust(src);
        let uses = UseMap::from_tree(tree.root_node(), src);
        let fn_item = find_function_item(tree.root_node(), src, name).expect(name);
        let body = extract_fn(fn_item, src, "input.rs", &uses);
        print_raw_fn(name, &body)
    }

    #[test]
    fn extract_if_else_if_else_headers() {
        let src = r#"
fn f(x: i32) {
    if x < 0 {
        return;
    } else if x == 0 {
        return;
    } else {
        return;
    }
}
"#;
        assert_eq!(
            outline_of(src, "f"),
            "\
fn f
  if x < 0
    return
  else if x == 0
    return
  else
    return
"
        );
    }

    #[test]
    fn extract_match_arms() {
        let src = r#"
fn f(x: i32) {
    match x {
        0 => return,
        1 | 2 => {
            foo();
        }
        n if n > 10 => return,
        _ => {}
    }
}
"#;
        assert_eq!(
            outline_of(src, "f"),
            "\
fn f
  match x
    0
      return
    1 | 2
      foo()
    n if n > 10
      return
    _
"
        );
    }

    #[test]
    fn extract_loop_while_for_headers() {
        let src = r#"
fn f(items: &[u8]) {
    loop {
        break;
    }
    while true {
        continue;
    }
    for item in items {
        return;
    }
}
"#;
        assert_eq!(
            outline_of(src, "f"),
            "\
fn f
  loop
    break
  while true
    continue
  for item in items
    return
"
        );
    }

    #[test]
    fn extract_let_emits_compute() {
        let src = r#"
fn f() {
    let x = compute();
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  compute()\n");
    }

    #[test]
    fn extract_println_omitted() {
        let src = r#"
fn f() {
    println!("x");
    return;
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  return\n");
    }

    #[test]
    fn extract_todo_kept() {
        let src = r#"
fn f() {
    todo!();
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  todo!()\n");
    }

    #[test]
    fn extract_condition_calls_not_emitted() {
        let src = r#"
fn f() {
    if items.is_empty() {
        return;
    }
    for x in items.iter() {
        return;
    }
    match compute() {
        _ => return,
    }
}
"#;
        assert_eq!(
            outline_of(src, "f"),
            "\
fn f
  if items.is_empty()
    return
  for x in items.iter()
    return
  match compute()
    _
      return
"
        );
    }

    #[test]
    fn extract_return_true_kept() {
        let src = r#"
fn valid(&self) -> bool {
    return true;
}
"#;
        assert_eq!(outline_of(src, "valid"), "fn valid\n  return true\n");
    }

    #[test]
    fn extract_try_else_skip_leading_comments() {
        let src = r#"
fn f() {
    try /* c */ {
        foo();
    }
    if x {
        return;
    } else /* c */ {
        bar();
    }
}
"#;
        assert_eq!(
            outline_of(src, "f"),
            "\
fn f
  try
    foo()
  if x
    return
  else
    bar()
"
        );
    }

    #[test]
    fn root_function_items_skips_nested_and_signatures() {
        let src = r#"
trait T { fn sig(&self); }
fn outer() {
    fn inner() {}
}
impl X {
    fn method() {}
}
"#;
        let tree = parse_rust(src);
        let names: Vec<String> = root_function_items(tree.root_node())
            .into_iter()
            .map(|n| fn_name(n, src))
            .collect();
        assert_eq!(names, vec!["outer", "method"]);
    }

    #[test]
    fn extract_if_std_fs_exists_keeps_std() {
        let src = r#"
fn f() {
    if std::fs::exists(p) {
        return;
    }
}
"#;
        assert_eq!(
            outline_of(src, "f"),
            "\
fn f
  if std::fs::exists(p)
    return
"
        );
    }
}
