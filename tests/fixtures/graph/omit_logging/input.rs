fn info() {
    return;
}

fn f() {
    println!("x");
    eprintln!("x");
    print!("x");
    eprint!("x");
    dbg!(1);
    log::warn!("empty");
    tracing::info!("t");
    info();
}
