use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;

use crate::collect::{collect_path, collect_stdin};
use crate::{
    diff_text, graph_files, outline_files, print_graph_html, print_graph_json, print_graph_mermaid,
    RunOutput, SeerError,
};

const HELP: &str = "\
Control-and-call outlines of source code

Usage: seer [--help] [--version]
       seer <PATH>
       seer tree [PATH]
       seer graph [--format mermaid|json|html] [PATH]
       seer diff [REV] [REV]
       seer diff-trees <A> <B>

  seer graph PATH          call graph (Mermaid)
  seer graph --format html PATH
                           self-contained HTML graph
  --format mermaid|json|html
                           graph output only; default mermaid
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphFormat {
    Mermaid,
    Json,
    Html,
}

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
        Some("graph") => {
            if let Some(out) = rest.get(1).and_then(|s| help_or_version(s)) {
                return Ok(out);
            }
            let (format, path) = parse_graph_args(&rest[1..])?;
            run_graph(path.as_deref(), format, stdin_is_terminal)
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
    Ok(RunOutput { stdout, exit: 0 })
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
    let files = collect_files_for_command(path, stdin_is_terminal)?;
    Ok(RunOutput {
        stdout: outline_files(&files),
        exit: 0,
    })
}

fn parse_graph_args(tokens: &[String]) -> Result<(GraphFormat, Option<String>), SeerError> {
    let mut format = GraphFormat::Mermaid;
    let mut path = None;
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t == "--format" {
            let v = tokens.get(i + 1).ok_or_else(|| {
                SeerError::Usage("--format requires mermaid, json, or html".into())
            })?;
            format = parse_graph_format(v)?;
            i += 2;
            continue;
        }
        if let Some(v) = t.strip_prefix("--format=") {
            format = parse_graph_format(v)?;
            i += 1;
            continue;
        }
        if t.starts_with('-') && t != "-" {
            return Err(SeerError::Usage(format!("unexpected argument: {t}")));
        }
        if path.is_some() {
            return Err(SeerError::Usage("unexpected arguments".into()));
        }
        path = Some(t.clone());
        i += 1;
    }
    Ok((format, path))
}

fn parse_graph_format(v: &str) -> Result<GraphFormat, SeerError> {
    match v {
        "mermaid" => Ok(GraphFormat::Mermaid),
        "json" => Ok(GraphFormat::Json),
        "html" => Ok(GraphFormat::Html),
        _ => Err(SeerError::Usage(format!(
            "invalid --format: {v} (expected mermaid, json, or html)"
        ))),
    }
}

fn run_graph(
    path: Option<&str>,
    format: GraphFormat,
    stdin_is_terminal: bool,
) -> Result<RunOutput, SeerError> {
    let files = collect_files_for_command(path, stdin_is_terminal)?;
    let graph = graph_files(&files);
    let stdout = match format {
        GraphFormat::Mermaid => print_graph_mermaid(&graph),
        GraphFormat::Json => print_graph_json(&graph),
        GraphFormat::Html => print_graph_html(&graph, &files),
    };
    Ok(RunOutput { stdout, exit: 0 })
}

fn collect_files_for_command(
    path: Option<&str>,
    stdin_is_terminal: bool,
) -> Result<Vec<(String, String)>, SeerError> {
    match path {
        None if stdin_is_terminal => Err(SeerError::Usage(
            "path required when stdin is a terminal".into(),
        )),
        None => collect_stdin(),
        Some(p) => collect_path(p),
    }
}
