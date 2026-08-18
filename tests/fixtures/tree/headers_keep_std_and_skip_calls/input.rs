fn f() {
    if std::fs::exists(p) {
        return;
    }
    for x in items.iter() {
        return;
    }
    match compute() {
        _ => return,
    }
}

fn compute() -> i32 {
    return 1;
}
