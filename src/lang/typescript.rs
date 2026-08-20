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

const IF_STMT: &str = "if_statement";
const FN_DECL: &str = "function_declaration";
const GEN_FN: &str = "generator_function_declaration";
const METHOD_DEF: &str = "method_definition";
const METHOD_SIG: &str = "method_signature";
const ABS_METHOD: &str = "abstract_method_signature";
const STMT_BLOCK: &str = "statement_block";

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
    if is_fn_like(node.kind()) {
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

fn is_fn_like(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "function_signature"
            | "method_definition"
            | "method_signature"
            | "abstract_method_signature"
    )
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
            "class_body"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration" => {
                return FnKind::Method;
            }
            "program" => return FnKind::Free,
            _ => cur = n.parent(),
        }
    }
    if node.kind() == METHOD_DEF || node.kind() == METHOD_SIG || node.kind() == ABS_METHOD {
        FnKind::Method
    } else {
        FnKind::Free
    }
}

fn walk(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    match node.kind() {
        "ERROR" => Vec::new(),
        "debugger_statement" => Vec::new(),
        "if_statement" => extract_if(node, ctx),
        "for_statement" | "for_in_statement" | "while_statement" | "do_statement" => {
            vec![RawNode::Control {
                text: header_before_field(node, "body", ctx.src),
                children: walk_field(node, "body", ctx),
            }]
        }
        "switch_statement" => vec![RawNode::Control {
            text: header_before_field(node, "body", ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        "switch_body" => walk_named(node, ctx),
        "switch_case" => vec![RawNode::Control {
            text: header_before_field(node, "body", ctx.src)
                .trim()
                .trim_end_matches(':')
                .to_string(),
            children: switch_case_children(node, ctx),
        }],
        "switch_default" => vec![RawNode::Control {
            text: "default".to_string(),
            children: walk_named(node, ctx),
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
            children: first_named_child_kind(node, &[STMT_BLOCK])
                .map(|b| walk(b, ctx))
                .unwrap_or_else(|| walk_named(node, ctx)),
        }],
        "call_expression" => emit_call(node, ctx),
        "new_expression" => emit_new(node, ctx),
        "expression_statement" => node
            .named_child(0)
            .map(|e| walk(e, ctx))
            .unwrap_or_default(),
        "lexical_declaration" | "variable_declaration" => walk_named(node, ctx),
        "variable_declarator" => node
            .child_by_field_name("value")
            .map(|v| walk(v, ctx))
            .unwrap_or_default(),
        "assignment_expression" | "augmented_assignment_expression" => node
            .child_by_field_name("right")
            .map(|v| walk(v, ctx))
            .unwrap_or_default(),
        "arrow_function" | "function_expression" | "generator_function" => node
            .child_by_field_name("body")
            .map(|b| walk(b, ctx))
            .unwrap_or_default(),
        "statement_block" => walk_named(node, ctx),
        FN_DECL | GEN_FN => vec![RawNode::NestedFn {
            name: fn_name(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        METHOD_DEF => vec![RawNode::NestedFn {
            name: fn_name(node, ctx.src),
            children: walk_field(node, "body", ctx),
        }],
        "jsx_element" | "jsx_self_closing_element" | "jsx_fragment" => walk_named(node, ctx),
        "jsx_expression" => walk_named(node, ctx),
        kind if is_ignored(kind) => Vec::new(),
        _ => walk_named(node, ctx),
    }
}

fn is_ignored(kind: &str) -> bool {
    matches!(
        kind,
        "import_statement"
            | "export_statement"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "comment"
            | "html_comment"
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
        let raw = header_before_field(current, "consequence", ctx.src);
        let text = if keyword == "else if" {
            if let Some(rest) = raw.strip_prefix("if") {
                format!("else if{rest}")
            } else {
                format!("else if {raw}")
            }
        } else {
            raw
        };
        let children = walk_field(current, "consequence", ctx);
        out.push(RawNode::Control { text, children });
        let Some(alt) = current.child_by_field_name("alternative") else {
            break;
        };
        let inner = if alt.kind() == "else_clause" {
            first_named_child_kind(alt, &[IF_STMT, STMT_BLOCK]).unwrap_or(alt)
        } else {
            alt
        };
        if inner.kind() == IF_STMT {
            keyword = "else if";
            current = inner;
            continue;
        }
        out.push(RawNode::Control {
            text: "else".to_string(),
            children: walk(inner, ctx),
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

fn switch_case_children(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    if node.child_by_field_name("body").is_some() {
        return walk_field(node, "body", ctx);
    }
    let mut out = Vec::new();
    let mut after_colon = false;
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if !after_colon {
                // first named is the value; rest are statements when no body field
                after_colon = true;
                continue;
            }
            out.extend(walk(child, ctx));
        }
    }
    out
}

fn emit_call(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let func = node.child_by_field_name("function");
    let kind = func
        .map(|f| call_kind(f, ctx.src))
        .unwrap_or(CallKind::Free { path: Vec::new() });
    emit_site(node, kind, ctx)
}

fn call_kind(function_node: Node, src: &str) -> CallKind {
    match function_node.kind() {
        "member_expression" | "optional_member_expression" => {
            let name = function_node
                .child_by_field_name("property")
                .map(|p| node_text(p, src).to_string())
                .unwrap_or_default();
            CallKind::Method { name }
        }
        _ => CallKind::Free {
            path: ident_path(function_node, src),
        },
    }
}

fn emit_new(node: Node, ctx: &Ctx) -> Vec<RawNode> {
    let path = node
        .child_by_field_name("constructor")
        .map(|c| ident_path(c, ctx.src))
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

pub fn collect_imports(root: Node, src: &str, file: &str, map: &mut UseMap) {
    walk_imports(root, src, file, map);
}

fn walk_imports(node: Node, src: &str, file: &str, map: &mut UseMap) {
    if node.kind() == "ERROR" {
        return;
    }
    if node.kind() == "import_statement" {
        parse_import(node, src, file, map);
        return;
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            walk_imports(child, src, file, map);
        }
    }
}

fn parse_import(node: Node, src: &str, file: &str, map: &mut UseMap) {
    let Some(source) = node.child_by_field_name("source") else {
        return;
    };
    let spec = node_text(source, src).trim_matches(['"', '\'', '`']);
    let Some(mod_path) = resolve_ts_spec(file, spec) else {
        return;
    };
    if let Some(clause) = node.named_child(0).filter(|n| n.kind() == "import_clause") {
        bind_clause(clause, &mod_path, src, map);
    }
}

fn bind_clause(clause: Node, mod_path: &[String], src: &str, map: &mut UseMap) {
    for i in 0..clause.named_child_count() {
        if let Some(child) = clause.named_child(i) {
            match child.kind() {
                "identifier" => {
                    // default import
                    let name = node_text(child, src).to_string();
                    let mut path = mod_path.to_vec();
                    path.push(name.clone());
                    map.insert_binding(name, path);
                }
                "named_imports" => {
                    for j in 0..child.named_child_count() {
                        if let Some(spec) = child.named_child(j) {
                            bind_specifier(spec, mod_path, src, map);
                        }
                    }
                }
                "namespace_import" => {
                    if let Some(alias) =
                        child.named_child(child.named_child_count().saturating_sub(1))
                    {
                        let name = node_text(alias, src).to_string();
                        map.insert_binding(name, mod_path.to_vec());
                    }
                }
                _ => {}
            }
        }
    }
}

fn bind_specifier(spec: Node, mod_path: &[String], src: &str, map: &mut UseMap) {
    if spec.kind() != "import_specifier" {
        return;
    }
    let imported = spec
        .child_by_field_name("name")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_default();
    let local = spec
        .child_by_field_name("alias")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_else(|| imported.clone());
    let mut path = mod_path.to_vec();
    path.push(imported);
    map.insert_binding(local, path);
}

/// Relative `./` / `../` specs only. Bare specifiers are external (`None`).
pub fn resolve_ts_spec(file: &str, spec: &str) -> Option<Vec<String>> {
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return None;
    }
    let mut dir = file_dir_module(file);
    for seg in spec.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                dir.pop()?;
            }
            rest => {
                let stem = rest
                    .strip_suffix(".ts")
                    .or_else(|| rest.strip_suffix(".tsx"))
                    .or_else(|| rest.strip_suffix(".js"))
                    .unwrap_or(rest);
                dir.push(stem.to_string());
            }
        }
    }
    Some(dir)
}

fn file_dir_module(file: &str) -> Vec<String> {
    let rel = file.strip_prefix("src/").unwrap_or(file);
    let mut parts: Vec<String> = rel
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let Some(name) = parts.pop() else {
        return Vec::new();
    };
    if name == "index.ts"
        || name == "index.tsx"
        || name == "index.js"
        || name == "mod.ts"
        || name == "main.ts"
    {
        return parts;
    }
    parts
}

pub fn module_path(file: &str) -> Vec<String> {
    if file == "<stdin>" {
        return Vec::new();
    }
    let rel = file.strip_prefix("src/").unwrap_or(file);
    let mut parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    let Some(file_name) = parts.pop() else {
        return Vec::new();
    };
    if file_name == "index.ts"
        || file_name == "index.tsx"
        || file_name == "index.js"
        || file_name == "mod.ts"
        || file_name == "main.ts"
    {
        return parts.into_iter().map(str::to_string).collect();
    }
    let stem = file_name
        .strip_suffix(".tsx")
        .or_else(|| file_name.strip_suffix(".ts"))
        .or_else(|| file_name.strip_suffix(".mts"))
        .or_else(|| file_name.strip_suffix(".cts"))
        .unwrap_or(file_name);
    parts.push(stem);
    parts.into_iter().map(str::to_string).collect()
}
