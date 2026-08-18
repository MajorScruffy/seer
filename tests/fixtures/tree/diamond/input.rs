fn a() {
    b();
    c();
}

fn b() {
    d();
}

fn c() {
    d();
}

fn d() {
    return;
}
