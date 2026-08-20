use tree_sitter::Node;

use crate::collapse::collapse_node;
use crate::ir::{CallKind, CallSite, FnDef, FnId, FnKind, RawNode};
use crate::lang::{first_named_child_kind, header_before_field, ident_path, node_text};
use crate::omit::{should_omit_at_extract, UseMap};

struct Ctx<'a> {
    src: &'a str,
    file: &'a str,
    uses: &'a UseMap,
}

const METHOD_DECL: &str = "method_declaration";
const CTOR_DECL: &str = "constructor_declaration";
const IF_STMT: &str = "if_statement";
const BLOCK: &str = "block";

pub fn index_defs(
    root: Node,
    src: &str,
    file: &str,
    uses: &UseMap,
    module: &[String],
) -> Vec<FnDef> {
    let mut out = Vec::new();
    walk_index(root, src, file, uses, module, false, &mut out);
    out
}

fn walk_index<'a>(
    node: Node<'a>,
    src: &str,
    file: &str,
    uses: &UseMap,
    module: &[String],
    in_fn: bool,
    out: &mut Vec<FnDef>,
) {
    if node.kind() == "ERROR" {
        return;
    }
    if node.kind() == METHOD_DECL || node.kind() == CTOR_DECL {
        let has_body = node.child_by_field_name("body").is_some();
        let body = if has_body {
            extract_fn(node, src, file, uses)
        } else {
            Vec::new()
        };
        out.push(FnDef {
            id: FnId {
                file: file.to_string(),
                start_byte: node.start_byte(),
            },
            name: fn_name(node, src),
            kind: classify(node),
            module: module.to_vec(),
            nested: in_fn,
            has_body,
            body,
        });
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                walk_index(child, src, file, uses, module, true, out);
            }
        }
        return;
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            walk_index(child, src, file, uses, module, in_fn, out);
        }
    }
}

pub fn extract_fn(fn_item: Node, src: &str, file: &str, uses: &UseMap) -> Vec<RawNode> {
    let ctx = Ctx { src, file, uses };
    fn_item
        .child_by_field_name("body")
        .map(|body| walk(body, &ctx))
        .unwrap_or_default()
}

fn fn_name(node: Node, src: &str) -> String {
    node.child_by_field_name("name")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_default()
}

fn classify(node: Node) -> FnKind {
    let mut cur = node.parent();
    while let Some(n) = cur {
        match n.kind() {
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
            | "enum_body_declarations" => return FnKind::Method,
            "program" => return FnKind::Free,
            _ => cur = n.parent(),
        }
    }
    FnKind::Method
}

fn walk(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    match node.kind() {
        "ERROR" => Vec::new(),
        "if_statement" => extract_if(node, ctx),
        "for_statement" | "enhanced_for_statement" | "while_statement" | "do_statement" => {
            vec![RawNode::Control {
                text: header_before_field(node, "body", ctx.src),
                children: walk_field(node, "body", ctx),
            }]
        }
        "switch_expression" | "switch_statement" => vec![RawNode::Control {
            text: header_before_field(node, "body", ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        "switch_block" => walk_named(node, ctx),
        "switch_block_statement_group" | "switch_rule" => vec![RawNode::Control {
            text: switch_label(node, ctx.src),
            children: walk_named_skip_labels(node, ctx),
        }],
        "return_statement" | "break_statement" | "continue_statement" | "throw_statement" => {
            vec![RawNode::Control {
                text: collapse_node(node, ctx.src)
                    .trim_end_matches(';')
                    .to_string(),
                children: Vec::new(),
            }]
        }
        "try_statement" => extract_try(node, ctx),
        "catch_clause" => vec![RawNode::Control {
            text: header_before_field(node, "body", ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        "finally_clause" => vec![RawNode::Control {
            text: "finally".to_string(),
            children: first_named_child_kind(node, &[BLOCK])
                .map(|b| walk(b, ctx))
                .unwrap_or_else(|| walk_named(node, ctx)),
        }],
        "method_invocation" => emit_invocation(node, ctx),
        "object_creation_expression" => emit_new(node, ctx),
        "expression_statement" => node
            .named_child(0)
            .map(|e| walk(e, ctx))
            .unwrap_or_default(),
        "local_variable_declaration" => walk_named(node, ctx),
        "variable_declarator" => node
            .child_by_field_name("value")
            .map(|v| walk(v, ctx))
            .unwrap_or_default(),
        "assignment_expression" => node
            .child_by_field_name("right")
            .map(|v| walk(v, ctx))
            .unwrap_or_default(),
        "lambda_expression" => node
            .child_by_field_name("body")
            .map(|b| walk(b, ctx))
            .unwrap_or_default(),
        METHOD_DECL | CTOR_DECL => vec![RawNode::NestedFn {
            name: fn_name(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        "block" => walk_named(node, ctx),
        "labeled_statement" => walk_named(node, ctx),
        kind if is_ignored(kind) => Vec::new(),
        _ => walk_named(node, ctx),
    }
}

fn is_ignored(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "import_declaration"
            | "package_declaration"
            | "field_declaration"
            | "annotation"
            | "marker_annotation"
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

fn extract_if(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let mut out = Vec::new();
    let mut keyword = "if";
    let mut current = node;
    loop {
        let text = header_before_field(current, "consequence", ctx.src);
        let text = if keyword == "else if" {
            if let Some(rest) = text.strip_prefix("if") {
                format!("else if{rest}")
            } else {
                format!("else if {text}")
            }
        } else {
            text
        };
        let children = walk_field(current, "consequence", ctx);
        out.push(RawNode::Control { text, children });
        let Some(alt) = current.child_by_field_name("alternative") else {
            break;
        };
        if alt.kind() == IF_STMT {
            keyword = "else if";
            current = alt;
            continue;
        }
        out.push(RawNode::Control {
            text: "else".to_string(),
            children: walk(alt, ctx),
        });
        break;
    }
    out
}

fn extract_try(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let mut out = vec![RawNode::Control {
        text: "try".to_string(),
        children: walk_field(node, "body", ctx),
    }];
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            match child.kind() {
                "catch_clause" | "finally_clause" => out.extend(walk(child, ctx)),
                _ => {}
            }
        }
    }
    out
}

fn switch_label(node: Node, src: &str) -> String {
    let mut parts = Vec::new();
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if child.kind() == "switch_label" {
                parts.push(collapse_node(child, src));
            }
        }
    }
    if parts.is_empty() {
        collapse_node(node, src)
    } else {
        parts.join(" ")
    }
}

fn walk_named_skip_labels(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let mut out = Vec::new();
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if child.kind() == "switch_label" {
                continue;
            }
            out.extend(walk(child, ctx));
        }
    }
    out
}

fn emit_invocation(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(n, ctx.src).to_string())
        .unwrap_or_default();
    let kind = if node.child_by_field_name("object").is_some() {
        CallKind::Method { name: name.clone() }
    } else {
        CallKind::Free {
            path: vec![name.clone()],
        }
    };
    emit_site(node, kind, ctx)
}

fn emit_new(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let path = node
        .child_by_field_name("type")
        .map(|t| ident_path(t, ctx.src))
        .unwrap_or_default();
    emit_site(node, CallKind::Free { path }, ctx)
}

fn emit_site(node: Node, kind: CallKind, ctx: &Ctx) -> Vec<RawNode> {
    let site = CallSite {
        display: collapse_node(node, ctx.src),
        kind,
        is_macro: false,
        file: ctx.file.to_string(),
    };
    if should_omit_at_extract(&site, ctx.uses) {
        Vec::new()
    } else {
        vec![RawNode::Call { site }]
    }
}

pub fn collect_imports(root: Node, src: &str, map: &mut UseMap) {
    walk_imports(root, src, map);
}

fn walk_imports(node: Node, src: &str, map: &mut UseMap) {
    if node.kind() == "ERROR" {
        return;
    }
    if node.kind() == "import_declaration" {
        parse_import(node, src, map);
        return;
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            walk_imports(child, src, map);
        }
    }
}

fn parse_import(node: Node, src: &str, map: &mut UseMap) {
    let mut path = Vec::new();
    let mut is_star = false;
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            match c.kind() {
                "asterisk" | "*" => is_star = true,
                "scoped_identifier" | "identifier" => path = ident_path(c, src),
                _ => {}
            }
        }
    }
    if path.is_empty() {
        return;
    }
    if is_star {
        map.push_glob(path);
        return;
    }
    let name = path.last().cloned().unwrap_or_default();
    map.insert_binding(name, path);
}

pub fn module_path_from_tree(root: Node, file: &str, src: &str) -> Vec<String> {
    let stem = std::path::Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Foo")
        .to_string();
    let mut pkg = Vec::new();
    for i in 0..root.named_child_count() {
        if let Some(child) = root.named_child(i) {
            if child.kind() == "package_declaration" {
                for j in 0..child.named_child_count() {
                    if let Some(n) = child.named_child(j) {
                        if n.kind() == "scoped_identifier" || n.kind() == "identifier" {
                            pkg = ident_path(n, src);
                        }
                    }
                }
            }
        }
    }
    pkg.push(stem);
    pkg
}
