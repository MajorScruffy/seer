fn f(x: i32) {
    match x {
        0 => return,
        1 | 2 => {
            foo();
        }
        n if n > 10 => return,
        _ => {}
    }
}

fn foo() {}
