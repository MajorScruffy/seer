fn f() {
    todo!();
    assert!(true);
    let _ = format!("a{}", 1);
    let _ = vec![1, 2];
    unwrap_now();
}

fn unwrap_now() {
    let x = Some(1);
    x.unwrap();
}
