fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match seer::run(&args) {
        Ok(out) => {
            print!("{}", out.stdout);
            out.exit
        }
        Err(err) => {
            eprintln!("error: {err}");
            err.exit_code()
        }
    };
    std::process::exit(code);
}
