fn walk(node: &Node) {
    if node.has_child() {
        walk(node.child());
    }
}
