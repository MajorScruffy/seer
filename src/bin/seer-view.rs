use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

const HELP: &str = "\
Side-by-side colored view of a seer flow-diff

Usage: seer-view [--help]
       seer-view
       seer-view <REV>
       seer-view <REV1> <REV2>
";

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let rest = args.get(1..).unwrap_or(&[]);
    match rest.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    if rest.len() > 2 {
        eprintln!("error: unexpected arguments");
        return ExitCode::from(2);
    }

    let mut seer_args = vec!["seer".to_string(), "diff".to_string()];
    seer_args.extend(rest.iter().cloned());

    let out = match seer::run(&seer_args) {
        Ok(out) => out,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(err.exit_code() as u8);
        }
    };
    if out.stdout.is_empty() {
        return ExitCode::from(out.exit as u8);
    }

    let color = io::stdout().is_terminal();
    let width = term_cols();
    let view = render_side_by_side(&out.stdout, width, color);
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(view.as_bytes());
    ExitCode::from(out.exit as u8)
}

fn term_cols() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n >= 20)
        .unwrap_or(80)
}

#[derive(Clone, Copy)]
enum Kind {
    Context,
    Change,
}

fn render_side_by_side(diff: &str, width: usize, color: bool) -> String {
    let col = ((width.saturating_sub(3)) / 2).max(8);
    let mut left_title = String::from("old");
    let mut right_title = String::from("new");
    let mut pairs: Vec<(Option<String>, Option<String>)> = Vec::new();

    let lines: Vec<&str> = if diff.is_empty() {
        Vec::new()
    } else {
        diff.split_inclusive('\n')
            .map(|l| l.strip_suffix('\n').unwrap_or(l))
            .collect()
    };

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(name) = line.strip_prefix("--- ") {
            left_title = name.to_string();
            i += 1;
            continue;
        }
        if let Some(name) = line.strip_prefix("+++ ") {
            right_title = name.to_string();
            i += 1;
            continue;
        }
        if line.starts_with("@@") {
            i += 1;
            continue;
        }
        if line.starts_with('-') && !line.starts_with("--- ") {
            let mut removed = Vec::new();
            while i < lines.len() && lines[i].starts_with('-') && !lines[i].starts_with("--- ") {
                removed.push(lines[i][1..].to_string());
                i += 1;
            }
            let mut added = Vec::new();
            while i < lines.len() && lines[i].starts_with('+') && !lines[i].starts_with("+++ ") {
                added.push(lines[i][1..].to_string());
                i += 1;
            }
            let n = removed.len().max(added.len());
            for k in 0..n {
                pairs.push((removed.get(k).cloned(), added.get(k).cloned()));
            }
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++ ") {
            pairs.push((None, Some(line[1..].to_string())));
            i += 1;
            continue;
        }
        let text = line.strip_prefix(' ').unwrap_or(line);
        pairs.push((Some(text.to_string()), Some(text.to_string())));
        i += 1;
    }

    let mut out = String::new();
    out.push_str(&format_row(
        &left_title,
        &right_title,
        col,
        color,
        Kind::Context,
    ));
    out.push_str(&format_rule(col, color));
    for (l, r) in pairs {
        let left = l.as_deref().unwrap_or("");
        let right = r.as_deref().unwrap_or("");
        let kind = if l.as_deref() == r.as_deref() {
            Kind::Context
        } else {
            Kind::Change
        };
        out.push_str(&format_row(left, right, col, color, kind));
    }
    out
}

fn format_rule(col: usize, color: bool) -> String {
    let line = format!("{}┄{}┄{}", "┄".repeat(col), "┄", "┄".repeat(col));
    if color {
        format!("{DIM}{line}{RESET}\n")
    } else {
        format!("{line}\n")
    }
}

fn format_row(left: &str, right: &str, col: usize, color: bool, kind: Kind) -> String {
    let l = pad_trunc(left, col);
    let r = pad_trunc(right, col);
    let (l, r) = if !color {
        (l, r)
    } else {
        match kind {
            Kind::Change => (
                if left.is_empty() {
                    format!("{DIM}{l}{RESET}")
                } else {
                    format!("{RED}{l}{RESET}")
                },
                if right.is_empty() {
                    format!("{DIM}{r}{RESET}")
                } else {
                    format!("{GREEN}{r}{RESET}")
                },
            ),
            Kind::Context => (l, r),
        }
    };
    format!("{l} │ {r}\n")
}

fn pad_trunc(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n > width {
        let take = width.saturating_sub(1);
        let mut out: String = s.chars().take(take).collect();
        out.push('…');
        out
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_minus_plus_side_by_side() {
        let diff = "\
--- HEAD
+++ WORKTREE
@@ -1,3 +1,4 @@
 process > handle
 fn handle
-  return
+  if true
+    return
";
        let out = render_side_by_side(diff, 40, false);
        assert!(out.contains("HEAD"));
        assert!(out.contains("WORKTREE"));
        assert!(out.contains("process > handle"));
        assert!(out.contains("  return"));
        assert!(out.contains("  if true"));
        assert!(out.contains("│"));
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn color_wraps_added_and_removed() {
        let diff = "--- a\n+++ b\n-old\n+new\n";
        let out = render_side_by_side(diff, 40, true);
        assert!(out.contains(RED));
        assert!(out.contains(GREEN));
        assert!(out.contains("old"));
        assert!(out.contains("new"));
    }
}
