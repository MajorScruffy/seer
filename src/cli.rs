use clap::{error::ErrorKind, CommandFactory, Parser};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;

use crate::collect::{collect_path, collect_stdin};
use crate::{diff_text, outline_files, RunOutput, SeerError};

#[derive(Parser)]
#[command(
    name = "seer",
    version,
    about = "Control-and-call outlines of source code",
    color = clap::ColorChoice::Never
)]
struct Cli {}

#[derive(Parser)]
#[command(
    name = "seer tree",
    version,
    about = "Print a control-and-call outline",
    color = clap::ColorChoice::Never
)]
struct TreeArgs {
    /// File, directory, or `-` for stdin
    #[arg(value_name = "PATH")]
    path: Option<String>,
}

#[derive(Parser)]
#[command(
    name = "seer diff-trees",
    version,
    about = "Diff two outline text files",
    color = clap::ColorChoice::Never
)]
struct DiffTreesArgs {
    /// Old outline text
    #[arg(value_name = "A")]
    a: String,
    /// New outline text
    #[arg(value_name = "B")]
    b: String,
}

#[derive(Parser)]
#[command(
    name = "seer diff",
    version,
    about = "Diff outlines of git revisions or the worktree",
    color = clap::ColorChoice::Never
)]
struct DiffArgs {
    /// Git revisions (0 = WORKTREE vs HEAD, 1 = WORKTREE vs REV, 2 = REV1 vs REV2)
    #[arg(value_name = "REV")]
    revs: Vec<String>,
}

pub(crate) fn run(args: &[String]) -> Result<RunOutput, SeerError> {
    run_with(args, std::io::stdin().is_terminal())
}

pub(crate) fn run_with(args: &[String], stdin_is_terminal: bool) -> Result<RunOutput, SeerError> {
    let rest = args.get(1..).unwrap_or(&[]);
    match rest.first().map(String::as_str) {
        Some("-h" | "--help") => Ok(RunOutput {
            stdout: Cli::command().render_help().to_string(),
            exit: 0,
        }),
        Some("-V" | "--version") => Ok(RunOutput {
            stdout: Cli::command().render_version().to_string(),
            exit: 0,
        }),
        Some("tree") => match parse_tree_args(&rest[1..])? {
            TreeParse::HelpOrVersion(out) => Ok(out),
            TreeParse::Args(tree) => run_tree(tree.path.as_deref(), stdin_is_terminal),
        },
        Some("diff-trees") => match parse_diff_trees_args(&rest[1..])? {
            DiffTreesParse::HelpOrVersion(out) => Ok(out),
            DiffTreesParse::Args(args) => run_diff_trees(&args.a, &args.b),
        },
        Some("diff") => match parse_diff_args(&rest[1..])? {
            DiffParse::HelpOrVersion(out) => Ok(out),
            DiffParse::Args(args) => crate::git::run(&args.revs),
        },
        None => crate::git::run(&[]),
        Some(path) => {
            if rest.len() > 1 {
                return Err(SeerError::Usage("unexpected arguments".into()));
            }
            run_tree(Some(path), stdin_is_terminal)
        }
    }
}

enum TreeParse {
    HelpOrVersion(RunOutput),
    Args(TreeArgs),
}

fn parse_tree_args(rest: &[String]) -> Result<TreeParse, SeerError> {
    let mut argv = vec!["seer".to_string()];
    argv.extend_from_slice(rest);
    match TreeArgs::try_parse_from(&argv) {
        Ok(args) => Ok(TreeParse::Args(args)),
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                Ok(TreeParse::HelpOrVersion(RunOutput {
                    stdout: err.to_string(),
                    exit: 0,
                }))
            }
            _ => Err(clap_usage(err)),
        },
    }
}

enum DiffTreesParse {
    HelpOrVersion(RunOutput),
    Args(DiffTreesArgs),
}

fn parse_diff_trees_args(rest: &[String]) -> Result<DiffTreesParse, SeerError> {
    let mut argv = vec!["seer".to_string()];
    argv.extend_from_slice(rest);
    match DiffTreesArgs::try_parse_from(&argv) {
        Ok(args) => Ok(DiffTreesParse::Args(args)),
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                Ok(DiffTreesParse::HelpOrVersion(RunOutput {
                    stdout: err.to_string(),
                    exit: 0,
                }))
            }
            _ => Err(clap_usage(err)),
        },
    }
}

enum DiffParse {
    HelpOrVersion(RunOutput),
    Args(DiffArgs),
}

fn parse_diff_args(rest: &[String]) -> Result<DiffParse, SeerError> {
    let mut argv = vec!["seer".to_string()];
    argv.extend_from_slice(rest);
    match DiffArgs::try_parse_from(&argv) {
        Ok(args) => Ok(DiffParse::Args(args)),
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                Ok(DiffParse::HelpOrVersion(RunOutput {
                    stdout: err.to_string(),
                    exit: 0,
                }))
            }
            _ => Err(clap_usage(err)),
        },
    }
}

fn clap_usage(err: clap::Error) -> SeerError {
    let msg = err.to_string();
    let msg = msg.strip_prefix("error: ").unwrap_or(&msg);
    SeerError::Usage(msg.to_string())
}

fn run_diff_trees(path_a: &str, path_b: &str) -> Result<RunOutput, SeerError> {
    let a = read_outline_text(path_a)?;
    let b = read_outline_text(path_b)?;
    let stdout = diff_text(&a, &b, path_a, path_b);
    let exit = if stdout.is_empty() { 0 } else { 1 };
    Ok(RunOutput { stdout, exit })
}

/// Read outline text as-is; do not parse as Rust.
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
