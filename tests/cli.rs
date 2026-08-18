use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_seer"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn tree_fixture(name: &str, file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tree")
        .join(name)
        .join(file)
}

fn expected(name: &str) -> Vec<u8> {
    std::fs::read(tree_fixture(name, "expected.txt")).unwrap()
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run seer")
}

fn run_stdin(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn seer");
    {
        let mut pipe = child.stdin.take().expect("stdin");
        pipe.write_all(stdin).expect("write stdin");
    }
    child.wait_with_output().expect("wait seer")
}

#[test]
fn cli_tree_subcommand() {
    let out = run(&["tree", "tests/fixtures/tree/process_handle/input.rs"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected("process_handle"));
}

#[test]
fn cli_tree_bare_path() {
    let out = run(&["tests/fixtures/tree/empty_file/input.rs"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn cli_tree_stdin_piped() {
    let src = std::fs::read(tree_fixture("process_handle", "input.rs")).unwrap();
    let out = run_stdin(&["tree"], &src);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected("process_handle"));
}

#[test]
fn cli_tree_dash() {
    let src = std::fs::read(tree_fixture("process_handle", "input.rs")).unwrap();
    let out = run_stdin(&["-"], &src);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected("process_handle"));
}

#[test]
fn cli_tree_directory() {
    let out = run(&["tree", "tests/fixtures/tree/name_collision/input"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, expected("name_collision"));
}

#[test]
fn cli_tree_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(&["tree", dir.path().to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn cli_tree_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.js");
    std::fs::write(&path, "console.log(1)\n").unwrap();
    let out = run(&["tree", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unsupported language"), "{err}");
}

#[test]
fn cli_tree_missing() {
    let out = run(&["tree", "/no/such/file.rs"]);
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("error:"), "{err}");
}

#[test]
fn cli_help_no_ansi() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.contains(&0x1b));
}

#[test]
fn cli_version() {
    let out = run(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.contains(&0x1b));
}
