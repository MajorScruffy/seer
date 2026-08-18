mod cli;
mod error;
mod git;

pub mod collapse;
pub mod collect;
pub mod diff;
pub mod entry;
pub mod expand;
pub mod extract;
pub mod ir;
pub mod lang;
pub mod omit;
pub mod parse;
pub mod print;
pub mod resolve;

pub use collapse::{collapse, collapse_node, strip_std};
pub use diff::diff_text;
pub use error::SeerError;
pub use ir::{CallKind, CallSite, FnDef, FnId, FnKind, Outline, OutlineNode, RawNode};
pub use print::print;

/// Successful CLI result. `exit` is 0 or 1.
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
    let index = resolve::index_files(files);
    let called = entry::called_targets(&index);
    let entries = entry::select_entries(&index, &called);
    let mut roots = Vec::new();
    for id in entries {
        let def = index.def(&id).expect("entry is indexed");
        let mut stack = Vec::new();
        let children = expand::expand_fn(def, &mut stack, &index);
        roots.push(ir::OutlineNode {
            text: format!("fn {}", def.name),
            children,
        });
    }
    print::print(&ir::Outline { roots })
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
            "fn a\n  fn inner\n    return\n  return\n\nfn b\n  return\n"
        );
    }

    #[test]
    fn diff_identical_empty() {
        assert_eq!(diff_text("fn a\n", "fn a\n", "a", "b"), "");
    }
}
