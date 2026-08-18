use clap::{error::ErrorKind, CommandFactory, Parser};
use std::io::IsTerminal;

use crate::collect::{collect_path, collect_stdin};
use crate::{outline_files, RunOutput, SeerError};

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
        Some("diff" | "diff-trees") | None => Err(SeerError::NotImplemented),
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

fn clap_usage(err: clap::Error) -> SeerError {
    let msg = err.to_string();
    let msg = msg.strip_prefix("error: ").unwrap_or(&msg);
    SeerError::Usage(msg.to_string())
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
