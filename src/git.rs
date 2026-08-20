use std::path::{Path, PathBuf};
use std::process::Command;

use crate::collect::collect_rs_files;
use crate::{diff_text, outline_files, RunOutput, SeerError};

/// One path for `seer` / `seer diff` / `seer diff REV` / `seer diff REV1 REV2`.
pub(crate) fn run(revs: &[String]) -> Result<RunOutput, SeerError> {
    let toplevel = require_repo()?;
    let (left, right, name_a, name_b) = match revs {
        [] => (
            collect_revision(&toplevel, "HEAD")?,
            collect_worktree(&toplevel)?,
            "HEAD",
            "WORKTREE",
        ),
        [rev] => (
            collect_revision(&toplevel, rev)?,
            collect_worktree(&toplevel)?,
            rev.as_str(),
            "WORKTREE",
        ),
        [rev1, rev2] => (
            collect_revision(&toplevel, rev1)?,
            collect_revision(&toplevel, rev2)?,
            rev1.as_str(),
            rev2.as_str(),
        ),
        _ => return Err(SeerError::Usage("too many revs".into())),
    };
    let stdout = diff_text(
        &outline_files(&left),
        &outline_files(&right),
        name_a,
        name_b,
    );
    let exit = if stdout.is_empty() { 0 } else { 1 };
    Ok(RunOutput { stdout, exit })
}

fn require_repo() -> Result<PathBuf, SeerError> {
    let inside = match Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
    {
        Ok(out) => out,
        Err(_) => return Err(not_a_repo()),
    };
    let is_true = inside.status.success()
        && std::str::from_utf8(&inside.stdout)
            .map(|s| s.trim() == "true")
            .unwrap_or(false);
    if !is_true {
        return Err(not_a_repo());
    }
    let top = git_stdout_cwd(&["rev-parse", "--show-toplevel"])?;
    let top = top.trim().trim_end_matches('/');
    Ok(PathBuf::from(top))
}

fn not_a_repo() -> SeerError {
    SeerError::Io("not a git repository".into())
}

fn verify_rev(toplevel: &Path, rev: &str) -> Result<(), SeerError> {
    let peeled = format!("{rev}^{{commit}}");
    let out = git_output(
        toplevel,
        &["rev-parse", "--verify", "--end-of-options", &peeled],
    )?;
    if !out.status.success() {
        return Err(SeerError::Io(format!("invalid revision: {rev}")));
    }
    Ok(())
}

fn collect_revision(toplevel: &Path, rev: &str) -> Result<Vec<(String, String)>, SeerError> {
    verify_rev(toplevel, rev)?;
    // Run from toplevel so names are worktree-relative, not cwd-relative.
    let listing = git_stdout(toplevel, &["ls-tree", "-r", "--name-only", rev])?;
    let mut files = Vec::new();
    for path in listing.lines() {
        if path.is_empty() || !keep_rs_path(path) {
            continue;
        }
        let spec = format!("{rev}:{path}");
        let bytes = git_bytes(toplevel, &["show", &spec])?;
        let src = String::from_utf8(bytes)
            .map_err(|_| SeerError::Io(format!("invalid utf-8: {path}")))?;
        files.push((path.to_string(), src));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// Disk walk from toplevel so a cwd of `src/` still sees root-level files.
fn collect_worktree(toplevel: &Path) -> Result<Vec<(String, String)>, SeerError> {
    collect_rs_files(toplevel)
}

fn keep_rs_path(posix: &str) -> bool {
    let mut last = "";
    for comp in posix.split('/') {
        if comp.is_empty() {
            continue;
        }
        if comp == "target" || comp == ".git" || comp == "node_modules" || comp.starts_with('.') {
            return false;
        }
        last = comp;
    }
    crate::lang::is_source_filename(last)
}

fn git_output(dir: &Path, args: &[&str]) -> Result<std::process::Output, SeerError> {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| SeerError::Io(format!("git: {e}")))
}

fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, SeerError> {
    let out = git_output(dir, args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim();
        if msg.is_empty() {
            return Err(SeerError::Io("git command failed".into()));
        }
        return Err(SeerError::Io(msg.to_string()));
    }
    Ok(out.stdout)
}

fn git_stdout(dir: &Path, args: &[&str]) -> Result<String, SeerError> {
    let bytes = git_bytes(dir, args)?;
    String::from_utf8(bytes).map_err(|_| SeerError::Io("invalid utf-8 from git".into()))
}

/// Discover the repo from the process cwd (may be a subdirectory).
fn git_stdout_cwd(args: &[&str]) -> Result<String, SeerError> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| SeerError::Io(format!("git: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim();
        if msg.is_empty() {
            return Err(SeerError::Io("git command failed".into()));
        }
        return Err(SeerError::Io(msg.to_string()));
    }
    String::from_utf8(out.stdout).map_err(|_| SeerError::Io("invalid utf-8 from git".into()))
}
