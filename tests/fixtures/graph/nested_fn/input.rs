fn outer() {
    fn inner() {
        return;
    }
    inner();
}

fn other() {
    fn unused() {
        return;
    }
    return;
}
