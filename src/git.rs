use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::collect::{file_is_under_any_path, is_source_file_path};
use crate::SeerError;

pub(crate) fn require_repo() -> Result<PathBuf, SeerError> {
    let inside = match run_git(None, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(out) => out,
        Err(_) => return Err(not_a_git_repository()),
    };
    let is_true = inside.status.success()
        && std::str::from_utf8(&inside.stdout)
            .map(|s| s.trim() == "true")
            .unwrap_or(false);
    if !is_true {
        return Err(not_a_git_repository());
    }
    let top = git_stdout_text(None, &["rev-parse", "--show-toplevel"])?;
    let top = top.trim().trim_end_matches('/');
    Ok(PathBuf::from(top))
}

fn not_a_git_repository() -> SeerError {
    SeerError::Io("not a git repository".into())
}

fn verify_rev(toplevel: &Path, rev: &str) -> Result<(), SeerError> {
    let peeled = format!("{rev}^{{commit}}");
    let out = run_git(
        Some(toplevel),
        &["rev-parse", "--verify", "--end-of-options", &peeled],
    )?;
    if !out.status.success() {
        return Err(SeerError::Io(format!("invalid revision: {rev}")));
    }
    Ok(())
}

pub(crate) fn collect_revision(
    toplevel: &Path,
    rev: &str,
    path_filters: &[String],
) -> Result<Vec<(String, String)>, SeerError> {
    verify_rev(toplevel, rev)?;
    let listing = list_paths_in_revision(toplevel, rev, path_filters)?;
    let mut paths = Vec::new();
    for path in listing.lines() {
        if path.is_empty()
            || !is_source_file_path(path)
            || !file_is_under_any_path(path, path_filters)
        {
            continue;
        }
        paths.push(path.to_string());
    }
    let mut files = read_blobs_in_revision(toplevel, rev, &paths)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn list_paths_in_revision(
    toplevel: &Path,
    rev: &str,
    path_filters: &[String],
) -> Result<String, SeerError> {
    let mut args: Vec<String> = vec![
        "ls-tree".into(),
        "-r".into(),
        "--name-only".into(),
        "--full-tree".into(),
        rev.into(),
    ];
    if !path_filters.is_empty() {
        args.push("--".into());
        args.extend(path_filters.iter().cloned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    git_stdout_text(Some(toplevel), &refs)
}

fn read_blobs_in_revision(
    toplevel: &Path,
    rev: &str,
    paths: &[String],
) -> Result<Vec<(String, String)>, SeerError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = String::new();
    for path in paths {
        input.push_str(rev);
        input.push(':');
        input.push_str(path);
        input.push('\n');
    }
    let bytes =
        git_output_bytes_with_stdin(Some(toplevel), &["cat-file", "--batch"], input.as_bytes())?;
    parse_blob_batch(&bytes, paths)
}

fn parse_blob_batch(bytes: &[u8], paths: &[String]) -> Result<Vec<(String, String)>, SeerError> {
    let mut pos = 0;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let rest = bytes.get(pos..).unwrap_or(&[]);
        let nl = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| SeerError::Io(format!("truncated git cat-file header: {path}")))?;
        let header = std::str::from_utf8(&rest[..nl])
            .map_err(|_| SeerError::Io(format!("invalid git cat-file header: {path}")))?;
        pos += nl + 1;
        if header.ends_with(" missing") {
            return Err(SeerError::Io(format!("missing blob: {path}")));
        }
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| SeerError::Io(format!("bad git cat-file header: {path}")))?;
        let end = pos
            .checked_add(size)
            .ok_or_else(|| SeerError::Io("blob too large".into()))?;
        let content = bytes
            .get(pos..end)
            .ok_or_else(|| SeerError::Io(format!("truncated git cat-file blob: {path}")))?;
        let src = String::from_utf8(content.to_vec())
            .map_err(|_| SeerError::Io(format!("invalid utf-8: {path}")))?;
        pos = end;
        if bytes.get(pos) != Some(&b'\n') {
            return Err(SeerError::Io(format!("missing LF after blob: {path}")));
        }
        pos += 1;
        files.push((path.clone(), src));
    }
    Ok(files)
}

fn run_git(dir: Option<&Path>, args: &[&str]) -> Result<std::process::Output, SeerError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.output().map_err(|e| SeerError::Io(format!("git: {e}")))
}

fn git_output_bytes(dir: Option<&Path>, args: &[&str]) -> Result<Vec<u8>, SeerError> {
    require_git_success(run_git(dir, args)?)
}

fn git_output_bytes_with_stdin(
    dir: Option<&Path>,
    args: &[&str],
    stdin: &[u8],
) -> Result<Vec<u8>, SeerError> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| SeerError::Io(format!("git: {e}")))?;
    {
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| SeerError::Io("git stdin".into()))?;
        pipe.write_all(stdin)
            .map_err(|e| SeerError::Io(format!("git: {e}")))?;
    }
    require_git_success(
        child
            .wait_with_output()
            .map_err(|e| SeerError::Io(format!("git: {e}")))?,
    )
}

fn require_git_success(out: std::process::Output) -> Result<Vec<u8>, SeerError> {
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

fn git_stdout_text(dir: Option<&Path>, args: &[&str]) -> Result<String, SeerError> {
    let bytes = git_output_bytes(dir, args)?;
    String::from_utf8(bytes).map_err(|_| SeerError::Io("invalid utf-8 from git".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_batch_blob_and_empty() {
        let mut bytes = b"deadbeef blob 3\n".to_vec();
        bytes.extend_from_slice(b"fn\n");
        bytes.push(b'\n');
        bytes.extend_from_slice(b"cafebabe blob 0\n\n");
        let files = parse_blob_batch(&bytes, &["a.rs".into(), "b.rs".into()]).unwrap();
        assert_eq!(
            files,
            vec![("a.rs".into(), "fn\n".into()), ("b.rs".into(), "".into())]
        );
    }
}
