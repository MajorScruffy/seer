pub mod java;
pub mod rust;
pub mod typescript;

use std::path::Path;

use tree_sitter::Node;

use crate::collapse::{collapse, literal_ranges};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    Java,
    TypeScript,
}

/// Extension → language. `<stdin>` is Rust. Unknown → `None`.
pub fn language_for_path(path: &str) -> Option<Language> {
    if path == "<stdin>" {
        return Some(Language::Rust);
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    if name.ends_with(".rs") {
        Some(Language::Rust)
    } else if name.ends_with(".java") {
        Some(Language::Java)
    } else if name.ends_with(".ts")
        || name.ends_with(".tsx")
        || name.ends_with(".mts")
        || name.ends_with(".cts")
    {
        Some(Language::TypeScript)
    } else {
        None
    }
}

pub fn is_source_filename(name: &str) -> bool {
    name.ends_with(".rs")
        || name.ends_with(".java")
        || name.ends_with(".ts")
        || name.ends_with(".tsx")
        || name.ends_with(".mts")
        || name.ends_with(".cts")
}

pub fn supported_languages_msg() -> &'static str {
    "v1 supports rust, java, and typescript"
}

/// Collapse source from `node.start` up to (not including) the named field.
pub fn header_before_field(node: Node, field: &str, src: &str) -> String {
    let start = node.start_byte();
    let end = node
        .child_by_field_name(field)
        .map(|b| b.start_byte())
        .unwrap_or(node.end_byte());
    if end <= start || end > src.len() {
        return crate::collapse::collapse_node(node, src);
    }
    let origin = start;
    let ranges: Vec<(usize, usize)> = literal_ranges(node)
        .into_iter()
        .filter_map(|(a, b)| {
            let a = a.min(end - origin);
            let b = b.min(end - origin);
            (a < b).then_some((a, b))
        })
        .collect();
    collapse(&src[start..end], &ranges)
}

pub fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

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

/// Identifier chain on `scoped_identifier` / `identifier` / `type_identifier`.
pub fn ident_path(node: Node, src: &str) -> Vec<String> {
    match node.kind() {
        "identifier" | "type_identifier" | "property_identifier" | "field_identifier" => {
            vec![node_text(node, src).to_string()]
        }
        "scoped_identifier" | "scoped_type_identifier" => {
            let mut segs = node
                .child_by_field_name("scope")
                .or_else(|| node.child_by_field_name("path"))
                .map(|p| ident_path(p, src))
                .unwrap_or_default();
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| node.named_child(node.named_child_count().saturating_sub(1)))
            {
                segs.extend(ident_path(name, src));
            }
            segs
        }
        "field_access" => {
            let mut segs = node
                .child_by_field_name("object")
                .map(|o| ident_path(o, src))
                .unwrap_or_default();
            if let Some(f) = node.child_by_field_name("field") {
                segs.extend(ident_path(f, src));
            }
            segs
        }
        "member_expression" => {
            let mut segs = node
                .child_by_field_name("object")
                .map(|o| ident_path(o, src))
                .unwrap_or_default();
            if let Some(p) = node.child_by_field_name("property") {
                segs.extend(ident_path(p, src));
            }
            segs
        }
        _ => Vec::new(),
    }
}
