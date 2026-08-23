use crate::lang::Language;

/// Parse UTF-8 Rust with tree-sitter-rust.
///
/// ERROR nodes are left in the tree; the process does not fail because of them.
pub fn parse_rust(src: &str) -> tree_sitter::Tree {
    parse(Language::Rust, src)
}

pub fn parse(lang: Language, src: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    match lang {
        Language::Rust => parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("load tree-sitter-rust"),
        Language::Java => parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .expect("load tree-sitter-java"),
        Language::TypeScript => {
            // TSX is a superset; .ts still parses. Callers pick LANGUAGE_TSX for .tsx.
            parser
                .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
                .expect("load tree-sitter-typescript");
        }
    }
    parser.parse(src, None).expect("parse source")
}

pub fn parse_path(path: &str, src: &str) -> tree_sitter::Tree {
    if path.ends_with(".tsx") {
        return parse_tsx(src);
    }
    match crate::lang::language_for_path(path) {
        Some(lang) => parse(lang, src),
        None => parse_rust(src),
    }
}

pub fn parse_tsx(src: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .expect("load tree-sitter-tsx");
    parser.parse(src, None).expect("parse tsx source")
}

/// Depth-first search for the first node of `kind`.
#[cfg(test)]
pub fn find_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(found) = find_kind(child, kind) {
                return Some(found);
            }
        }
    }
    None
}
