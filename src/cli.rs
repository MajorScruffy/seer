use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;

use crate::collect::{collect_path, collect_stdin, collect_worktree, repo_relative_paths};
use crate::{
    diff_text, graph_files, outline_diff, outline_files, print_graph_html, print_graph_json,
    print_graph_mermaid, RunOutput, SeerError,
};

const HELP: &str = "\
Control-and-call outlines of source code

Usage: seer [--help] [--version] [--max-lines N]
       seer <PATH>
       seer tree [PATH]
       seer graph [--format mermaid|json|html] [PATH]
       seer diff [REV] [REV] [--] [PATH...]
       seer -- [PATH...]
       seer diff-trees <A> <B>

  seer <file.rs|java|ts>   outline one file (path:line jump targets)
  seer <dir>               outline a directory
  seer                     outline-diff dirty worktree vs HEAD
  seer -- src/foo.rs       outline-diff that path only
  seer graph PATH          call graph (Mermaid)
  seer graph --format html PATH
                           self-contained HTML graph
  seer diff REV1 REV2      outline-diff two git revisions
  seer diff REV1 REV2 -- p outline-diff two revs, path-limited
  --max-lines N            cap stdout; last line is a truncation notice
  --format mermaid|json|html
                           graph output only; default mermaid

Exit 0 on success, including a printed diff. Empty stdout means no outline
change. Exit 2 usage, 3 runtime. Diffs never use exit 1.

Example:
  input.rs:1 fn process
    if items.is_empty()
      return
    for item in items
      if item.valid()
        input.rs:13 handle(item)
          if !item.ready()
            return
          serde_json::to_string(item)
          fs::write(path, data)
";

enum Cmd {
    Help,
    Version,
    Tree {
        path: Option<String>,
        max_lines: Option<usize>,
    },
    Diff {
        revs: Vec<String>,
        paths: Vec<String>,
        max_lines: Option<usize>,
    },
    DiffTrees {
        a: String,
        b: String,
        max_lines: Option<usize>,
    },
    Graph {
        path: Option<String>,
        format: GraphFormat,
        max_lines: Option<usize>,
    },
}

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

fn help_or_version(arg: &str) -> Option<Cmd> {
    match arg {
        "-h" | "--help" => Some(Cmd::Help),
        "-V" | "--version" => Some(Cmd::Version),
        _ => None,
    }
}

pub(crate) fn run_with(args: &[String], stdin_is_terminal: bool) -> Result<RunOutput, SeerError> {
    match parse_command(args)? {
        Cmd::Help => Ok(help()),
        Cmd::Version => Ok(version()),
        Cmd::Tree { path, max_lines } => {
            apply_line_limit(run_tree(path.as_deref(), stdin_is_terminal)?, max_lines)
        }
        Cmd::Diff {
            revs,
            paths,
            max_lines,
        } => apply_line_limit(run_diff(revs, paths)?, max_lines),
        Cmd::DiffTrees { a, b, max_lines } => apply_line_limit(run_diff_trees(&a, &b)?, max_lines),
        Cmd::Graph {
            path,
            format,
            max_lines,
        } => apply_line_limit(
            run_graph(path.as_deref(), format, stdin_is_terminal)?,
            max_lines,
        ),
    }
}

fn apply_line_limit(mut out: RunOutput, max_lines: Option<usize>) -> Result<RunOutput, SeerError> {
    if let Some(n) = max_lines {
        out.stdout = truncate_to_line_limit(&out.stdout, n);
    }
    Ok(out)
}

fn parse_command(args: &[String]) -> Result<Cmd, SeerError> {
    let rest = args.get(1..).unwrap_or(&[]);
    let (max_lines, tokens) = take_max_lines(rest)?;
    match tokens.first().map(String::as_str) {
        Some("-h" | "--help") => Ok(Cmd::Help),
        Some("-V" | "--version") => Ok(Cmd::Version),
        Some("tree") => {
            if let Some(cmd) = tokens.get(1).and_then(|s| help_or_version(s)) {
                return Ok(cmd);
            }
            match tokens.len() {
                1 => Ok(Cmd::Tree {
                    path: None,
                    max_lines,
                }),
                2 => Ok(Cmd::Tree {
                    path: Some(tokens[1].clone()),
                    max_lines,
                }),
                _ => Err(SeerError::Usage("unexpected arguments".into())),
            }
        }
        Some("graph") => {
            if let Some(cmd) = tokens.get(1).and_then(|s| help_or_version(s)) {
                return Ok(cmd);
            }
            parse_graph(&tokens[1..], max_lines)
        }
        Some("diff-trees") => {
            if let Some(cmd) = tokens.get(1).and_then(|s| help_or_version(s)) {
                return Ok(cmd);
            }
            if tokens.len() != 3 {
                return Err(SeerError::Usage("diff-trees requires two paths".into()));
            }
            Ok(Cmd::DiffTrees {
                a: tokens[1].clone(),
                b: tokens[2].clone(),
                max_lines,
            })
        }
        Some("diff") => {
            if let Some(cmd) = tokens.get(1).and_then(|s| help_or_version(s)) {
                return Ok(cmd);
            }
            let (revs, paths) = parse_diff_args(&tokens[1..])?;
            Ok(Cmd::Diff {
                revs,
                paths,
                max_lines,
            })
        }
        None => Ok(Cmd::Diff {
            revs: Vec::new(),
            paths: Vec::new(),
            max_lines,
        }),
        Some("--") => {
            let (revs, paths) = parse_diff_args(&tokens)?;
            Ok(Cmd::Diff {
                revs,
                paths,
                max_lines,
            })
        }
        Some(path) => {
            if tokens.len() > 1 {
                return Err(SeerError::Usage("unexpected arguments".into()));
            }
            Ok(Cmd::Tree {
                path: Some(path.to_string()),
                max_lines,
            })
        }
    }
}

fn take_max_lines(args: &[String]) -> Result<(Option<usize>, Vec<String>), SeerError> {
    let mut max_lines = None;
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--max-lines" {
            let n = args
                .get(i + 1)
                .ok_or_else(|| SeerError::Usage("--max-lines requires a number".into()))?;
            max_lines = Some(parse_max_lines(n)?);
            i += 2;
            continue;
        }
        if let Some(n) = a.strip_prefix("--max-lines=") {
            max_lines = Some(parse_max_lines(n)?);
            i += 1;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    Ok((max_lines, out))
}

fn parse_max_lines(s: &str) -> Result<usize, SeerError> {
    let n: usize = s
        .parse()
        .map_err(|_| SeerError::Usage(format!("invalid --max-lines: {s}")))?;
    if n == 0 {
        return Err(SeerError::Usage("--max-lines must be >= 1".into()));
    }
    Ok(n)
}

fn truncate_to_line_limit(s: &str, max: usize) -> String {
    let count = s.lines().count();
    if count <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i >= max {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!(
        "... truncated ({max}/{count} lines; pass a path or raise --max-lines)\n"
    ));
    out
}

/// Split `diff` argv into at most two revs and optional path filters.
/// `--` always starts paths. Without it, an existing cwd path is a path filter;
/// anything else is a rev (verified later).
fn parse_diff_args(args: &[String]) -> Result<(Vec<String>, Vec<String>), SeerError> {
    if let Some(i) = args.iter().position(|a| a == "--") {
        let revs = args[..i].to_vec();
        if revs.len() > 2 {
            return Err(SeerError::Usage("too many revs".into()));
        }
        return Ok((revs, args[i + 1..].to_vec()));
    }
    let mut revs = Vec::new();
    let mut paths = Vec::new();
    for arg in args {
        if paths.is_empty() && revs.len() < 2 && !Path::new(arg).exists() {
            revs.push(arg.clone());
        } else {
            paths.push(arg.clone());
        }
    }
    Ok((revs, paths))
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

fn parse_graph(tokens: &[String], max_lines: Option<usize>) -> Result<Cmd, SeerError> {
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
    Ok(Cmd::Graph {
        path,
        format,
        max_lines,
    })
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

fn run_tree(path: Option<&str>, stdin_is_terminal: bool) -> Result<RunOutput, SeerError> {
    let files = collect_files_for_command(path, stdin_is_terminal)?;
    Ok(RunOutput {
        stdout: outline_files(&files),
        exit: 0,
    })
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

fn run_diff(revs: Vec<String>, paths: Vec<String>) -> Result<RunOutput, SeerError> {
    let toplevel = crate::git::require_repo()?;
    let cwd = env::current_dir().map_err(|e| SeerError::Io(format!("cwd: {e}")))?;
    let path_filters = repo_relative_paths(&cwd, &toplevel, &paths)?;
    let stdout = match revs.as_slice() {
        [] => {
            let left = crate::git::collect_revision(&toplevel, "HEAD", &path_filters)?;
            let right = collect_worktree(&toplevel, &path_filters)?;
            outline_diff(&left, &right, "HEAD", "WORKTREE")
        }
        [rev] => {
            let left = crate::git::collect_revision(&toplevel, rev, &path_filters)?;
            let right = collect_worktree(&toplevel, &path_filters)?;
            outline_diff(&left, &right, rev, "WORKTREE")
        }
        [rev1, rev2] => {
            let left = crate::git::collect_revision(&toplevel, rev1, &path_filters)?;
            let right = crate::git::collect_revision(&toplevel, rev2, &path_filters)?;
            outline_diff(&left, &right, rev1, rev2)
        }
        _ => return Err(SeerError::Usage("too many revs".into())),
    };
    Ok(RunOutput { stdout, exit: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_vec(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_string()).collect()
    }

    #[test]
    fn truncate_keeps_short_output() {
        assert_eq!(truncate_to_line_limit("a\nb\n", 2), "a\nb\n");
    }

    #[test]
    fn truncate_adds_notice() {
        let out = truncate_to_line_limit("a\nb\nc\n", 2);
        assert_eq!(
            out,
            "a\nb\n... truncated (2/3 lines; pass a path or raise --max-lines)\n"
        );
    }

    #[test]
    fn max_lines_skips_help_and_version() {
        let help = run_with(
            &[
                "seer".into(),
                "--max-lines".into(),
                "2".into(),
                "--help".into(),
            ],
            true,
        )
        .unwrap();
        assert_eq!(help.exit, 0);
        assert_eq!(help.stdout, HELP);
        let ver = run_with(
            &[
                "seer".into(),
                "--max-lines".into(),
                "1".into(),
                "--version".into(),
            ],
            true,
        )
        .unwrap();
        assert_eq!(ver.exit, 0);
        assert!(!ver.stdout.contains("truncated"));
        assert!(ver.stdout.starts_with("seer "));
    }

    #[test]
    fn parse_dashdash_revs_and_paths() {
        assert_eq!(
            parse_diff_args(&args_vec(&["HEAD", "--", "src.rs"])).unwrap(),
            (vec!["HEAD".into()], vec!["src.rs".into()])
        );
        assert_eq!(
            parse_diff_args(&args_vec(&["--", "a.rs", "b.rs"])).unwrap(),
            (vec![], vec!["a.rs".into(), "b.rs".into()])
        );
        assert!(parse_diff_args(&args_vec(&["a", "b", "c", "--"])).is_err());
    }

    #[test]
    fn parse_no_dashdash_unknown_is_rev() {
        assert_eq!(
            parse_diff_args(&args_vec(&["not-a-rev"])).unwrap(),
            (vec!["not-a-rev".into()], vec![])
        );
    }
}
