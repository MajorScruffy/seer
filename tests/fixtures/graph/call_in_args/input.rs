fn outer() {
    wrap(inner());
}

fn wrap(_x: i32) {
    return;
}

fn inner() -> i32 {
    return 1;
}
