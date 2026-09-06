use seer::{graph_files, outline_files, print_graph_html, print_graph_json, print_graph_mermaid};
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

fn outline_path(path: &str) -> Vec<u8> {
    let src = std::fs::read_to_string(path).unwrap();
    outline_files(&[(path.to_string(), src)]).into_bytes()
}

fn outline_stdin(src: &[u8]) -> Vec<u8> {
    let src = String::from_utf8(src.to_vec()).unwrap();
    outline_files(&[("<stdin>".into(), src)]).into_bytes()
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
    let path = "tests/fixtures/tree/process_handle/input.rs";
    let out = run(&["tree", path]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, outline_path(path));
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
    assert_eq!(out.stdout, outline_stdin(&src));
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
    assert_eq!(out.stdout, outline_stdin(&src));
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
fn cli_tree_java() {
    let out = run(&["tree", "tests/fixtures/tree/java_process_handle/input.java"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        outline_path("tests/fixtures/tree/java_process_handle/input.java")
    );
}

#[test]
fn cli_tree_typescript() {
    let out = run(&["tree", "tests/fixtures/tree/ts_process_handle/input.ts"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        outline_path("tests/fixtures/tree/ts_process_handle/input.ts")
    );
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
fn cli_diff_trees_simple() {
    let out = run(&[
        "diff-trees",
        "tests/fixtures/diff/simple/a.txt",
        "tests/fixtures/diff/simple/b.txt",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = "\
--- tests/fixtures/diff/simple/a.txt
+++ tests/fixtures/diff/simple/b.txt
@@ -2,4 +2,5 @@
   if items.is_empty()
     return
   for item in items
-    handle(item)
+    if item.valid()
+      handle(item)
";
    assert_eq!(out.stdout, expected.as_bytes());
}

#[test]
fn cli_diff_trees_identical() {
    let out = run(&[
        "diff-trees",
        "tests/fixtures/diff/identical/a.txt",
        "tests/fixtures/diff/identical/b.txt",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn cli_help_no_ansi() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.contains(&0x1b));
}

#[test]
fn cli_help_has_graph() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.contains(&0x1b));
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("graph"), "{help}");
    assert!(help.contains("--format"), "{help}");
}

#[test]
fn cli_graph_subcommand() {
    let path = "tests/fixtures/tree/process_handle/input.rs";
    let out = run(&["graph", path]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("flowchart TD\n"), "{stdout}");
    assert!(stdout.contains(&format!("{path}:1 process")), "{stdout}");
    let src = std::fs::read_to_string(path).unwrap();
    let graph = graph_files(&[(path.to_string(), src)]);
    assert_eq!(out.stdout, print_graph_mermaid(&graph).as_bytes());
}

#[test]
fn cli_graph_json() {
    let out = run(&[
        "graph",
        "--format",
        "json",
        "tests/fixtures/tree/empty_file/input.rs",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn cli_graph_html() {
    let out = run(&[
        "graph",
        "--format",
        "html",
        "tests/fixtures/tree/empty_file/input.rs",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn cli_graph_html_process_handle() {
    let path = "tests/fixtures/tree/process_handle/input.rs";
    let out = run(&["graph", "--format", "html", path]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let src = std::fs::read_to_string(path).unwrap();
    let files = [(path.to_string(), src)];
    let graph = graph_files(&files);
    assert_eq!(out.stdout, print_graph_html(&graph, &files).as_bytes());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("<!DOCTYPE html>\n"));
    assert!(!stdout.contains("<script src"));
    assert_eq!(
        run(&["graph", path, "--format=json"]).stdout,
        print_graph_json(&graph).as_bytes()
    );
}

#[test]
fn cli_graph_bad_format() {
    let out = run(&["graph", "--format", "dot", "x.rs"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_graph_no_path_tty() {
    let err = seer::run_with(&["seer".into(), "graph".into()], true).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn cli_version() {
    let out = run(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.contains(&0x1b));
}
