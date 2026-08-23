use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;

use crate::collect::{collect_path, collect_stdin};
use crate::{diff_text, outline_files, RunOutput, SeerError};

const HELP: &str = "\
Control-and-call outlines of source code

Usage: seer [--help] [--version]
       seer <PATH>
       seer tree [PATH]
       seer diff [REV] [REV]
       seer diff-trees <A> <B>
";

pub(crate) fn run(args: &[String]) -> Result<RunOutput, SeerError> {
    run_with(args, std::io::stdin().is_terminal())
}

fn help() -> RunOutput {
    RunOutput {
        stdout: HELP.to_string(),
        exit: 0,
    }
}

fn version() -> RunOutput {
    RunOutput {
        stdout: format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        exit: 0,
    }
}

fn help_or_version(arg: &str) -> Option<RunOutput> {
    match arg {
        "-h" | "--help" => Some(help()),
        "-V" | "--version" => Some(version()),
        _ => None,
    }
}

pub(crate) fn run_with(args: &[String], stdin_is_terminal: bool) -> Result<RunOutput, SeerError> {
    let rest = args.get(1..).unwrap_or(&[]);
    match rest.first().map(String::as_str) {
        Some("-h" | "--help") => Ok(help()),
        Some("-V" | "--version") => Ok(version()),
        Some("tree") => {
            if let Some(out) = rest.get(1).and_then(|s| help_or_version(s)) {
                return Ok(out);
            }
            match rest.len() {
                1 => run_tree(None, stdin_is_terminal),
                2 => run_tree(Some(rest[1].as_str()), stdin_is_terminal),
                _ => Err(SeerError::Usage("unexpected arguments".into())),
            }
        }
        Some("diff-trees") => {
            if let Some(out) = rest.get(1).and_then(|s| help_or_version(s)) {
                return Ok(out);
            }
            if rest.len() != 3 {
                return Err(SeerError::Usage("diff-trees requires two paths".into()));
            }
            run_diff_trees(&rest[1], &rest[2])
        }
        Some("diff") => {
            if let Some(out) = rest.get(1).and_then(|s| help_or_version(s)) {
                return Ok(out);
            }
            if rest.len() > 3 {
                return Err(SeerError::Usage("too many revs".into()));
            }
            crate::git::run(&rest[1..])
        }
        None => crate::git::run(&[]),
        Some(path) => {
            if rest.len() > 1 {
                return Err(SeerError::Usage("unexpected arguments".into()));
            }
            run_tree(Some(path), stdin_is_terminal)
        }
    }
}

fn run_diff_trees(path_a: &str, path_b: &str) -> Result<RunOutput, SeerError> {
    let a = read_outline_text(path_a)?;
    let b = read_outline_text(path_b)?;
    let stdout = diff_text(&a, &b, path_a, path_b);
    let exit = if stdout.is_empty() { 0 } else { 1 };
    Ok(RunOutput { stdout, exit })
}

fn read_outline_text(path: &str) -> Result<String, SeerError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(SeerError::Io(format!("path not found: {path}")));
        }
        Err(e) => return Err(SeerError::Io(format!("{}: {e}", Path::new(path).display()))),
    };
    String::from_utf8(bytes).map_err(|_| SeerError::Io(format!("invalid utf-8: {path}")))
}

fn run_tree(path: Option<&str>, stdin_is_terminal: bool) -> Result<RunOutput, SeerError> {
    let files = match path {
        None if stdin_is_terminal => {
            return Err(SeerError::Usage(
                "path required when stdin is a terminal".into(),
            ));
        }
        None => collect_stdin()?,
        Some(p) => collect_path(p)?,
    };
    Ok(RunOutput {
        stdout: outline_files(&files),
        exit: 0,
    })
}
