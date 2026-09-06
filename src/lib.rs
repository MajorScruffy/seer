mod cli;
mod collapse;
mod collect;
mod diff;
mod error;
mod extract;
mod git;
mod graph;
mod ir;
mod lang;
mod omit;
mod parse;
mod resolve;

pub use collect::collect_source_files;
pub use diff::diff_text;
pub use error::SeerError;
pub use graph::{
    graph_files, print_graph_html, print_graph_json, print_graph_mermaid, CallGraph, GraphEdge,
    GraphNode,
};

/// Successful CLI result. `exit` is 0 on success (including a printed diff).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutput {
    pub stdout: String,
    pub exit: i32,
}

pub fn run(args: &[String]) -> Result<RunOutput, SeerError> {
    cli::run(args)
}

/// `stdin_is_terminal` is injected so `cli_tree_no_path_tty` does not need a pty.
pub fn run_with(args: &[String], stdin_is_terminal: bool) -> Result<RunOutput, SeerError> {
    cli::run_with(args, stdin_is_terminal)
}

/// `files` is (posix_relpath, rust_source_utf8). Sorted here if needed.
/// Does **not** read the disk and does **not** take Cargo.toml bytes (v1).
pub fn outline_files(files: &[(String, String)]) -> String {
    ir::print_outline(
        &resolve::outline(&resolve::index_files(files), resolve::Expand::Every),
        ir::Label::Jump,
    )
}

/// Diff two analyzed sets as entry-rooted flow trees (each callee once).
pub fn outline_diff(
    left: &[(String, String)],
    right: &[(String, String)],
    name_a: &str,
    name_b: &str,
) -> String {
    diff_text(
        &outline_for_diff(left),
        &outline_for_diff(right),
        name_a,
        name_b,
    )
}

fn outline_for_diff(files: &[(String, String)]) -> String {
    ir::print_outline(
        &resolve::outline(&resolve::index_files(files), resolve::Expand::Once),
        ir::Label::Flow,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_has_no_ansi() {
        let out = run(&["seer".into(), "--help".into()]).unwrap();
        assert_eq!(out.exit, 0);
        assert!(!out.stdout.as_bytes().contains(&0x1b));
        assert!(!out.stdout.is_empty());
    }

    #[test]
    fn version_has_no_ansi() {
        let out = run(&["seer".into(), "--version".into()]).unwrap();
        assert_eq!(out.exit, 0);
        assert!(!out.stdout.as_bytes().contains(&0x1b));
        assert!(out.stdout.contains("seer"));
    }

    #[test]
    fn cli_tree_no_path_tty() {
        let err = run_with(&["seer".into(), "tree".into()], true).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn outline_files_empty_input() {
        assert_eq!(outline_files(&[]), "");
    }

    #[test]
    fn outline_files_sorts_by_path_and_skips_nested() {
        let b = ("b.rs".into(), "fn b() { return; }\n".into());
        let a = (
            "a.rs".into(),
            "trait T { fn sig(&self); }\nfn a() { fn inner() { return; } return; }\n".into(),
        );
        assert_eq!(
            outline_files(&[b, a]),
            "a.rs:2 fn a\n  fn inner\n    return\n  return\n\nb.rs:1 fn b\n  return\n"
        );
    }

    #[test]
    fn diff_identical_empty() {
        assert_eq!(diff_text("fn a\n", "fn a\n", "a", "b"), "");
    }

    #[test]
    fn outline_diff_nested_callee() {
        let left = [(
            "a.rs".into(),
            "fn process() { handle(); }\nfn handle() { return; }\n".into(),
        )];
        let right = [(
            "a.rs".into(),
            "fn process() { handle(); }\nfn handle() { if true { return; } }\n".into(),
        )];
        assert_eq!(
            outline_diff(&left, &right, "a", "b"),
            "\
--- a
+++ b
@@ -1,3 +1,4 @@
 a.rs fn process
   a.rs handle()
-    return
+    if true
+      return
"
        );
    }

    #[test]
    fn outline_diff_move_fn_empty() {
        let left = [(
            "a.rs".into(),
            "fn process() { handle(); }\nfn handle() { return; }\n".into(),
        )];
        let right = [(
            "a.rs".into(),
            "fn handle() { return; }\nfn process() { handle(); }\n".into(),
        )];
        assert_eq!(outline_diff(&left, &right, "a", "b"), "");
    }

    #[test]
    fn outline_diff_new_call() {
        let left = [(
            "a.rs".into(),
            "fn process() { handle(); }\nfn handle() { return; }\n".into(),
        )];
        let right = [(
            "a.rs".into(),
            "fn process() { audit(); handle(); }\nfn handle() { return; }\nfn audit() { return; }\n"
                .into(),
        )];
        assert_eq!(
            outline_diff(&left, &right, "a", "b"),
            "\
--- a
+++ b
@@ -1,3 +1,5 @@
 a.rs fn process
+  a.rs audit()
+    return
   a.rs handle()
     return
"
        );
    }

    #[test]
    fn outline_diff_new_entry() {
        let left = [("a.rs".into(), "fn main() { return; }\n".into())];
        let right = [(
            "a.rs".into(),
            "fn main() { return; }\nfn extra() { return; }\n".into(),
        )];
        assert_eq!(
            outline_diff(&left, &right, "HEAD", "WORKTREE"),
            "\
--- HEAD
+++ WORKTREE
@@ -1,2 +1,5 @@
+a.rs fn extra
+  return
+
 a.rs fn main
   return
"
        );
    }

    #[test]
    fn outline_diff_expand_once() {
        let left = [(
            "a.rs".into(),
            "fn a() { b(); c(); }\nfn b() { d(); }\nfn c() { d(); }\nfn d() { return; }\n".into(),
        )];
        let right = [(
            "a.rs".into(),
            "fn a() { b(); c(); }\nfn b() { d(); }\nfn c() { d(); }\nfn d() { todo!(); }\n".into(),
        )];
        assert_eq!(
            outline_diff(&left, &right, "a", "b"),
            "\
--- a
+++ b
@@ -1,6 +1,6 @@
 a.rs fn a
   a.rs b()
     a.rs d()
-      return
+      todo!()
   a.rs c()
     a.rs d()
"
        );
    }
}
