/// Parse UTF-8 Rust with tree-sitter-rust.
///
/// ERROR nodes are left in the tree; the process does not fail because of them.
pub fn parse_rust(src: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("load tree-sitter-rust");
    parser.parse(src, None).expect("parse rust source")
}

/// Depth-first search for the first node of `kind`.
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
