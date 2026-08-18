use std::fs;
use std::path::{Path, PathBuf};

use crate::SeerError;

/// Regular `.rs` files under `root` as `(posix_relpath, utf8_source)`, sorted.
pub fn collect_rs_files(root: &Path) -> Result<Vec<(String, String)>, SeerError> {
    let mut files = Vec::new();
    walk(root, Path::new(""), &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn walk(abs: &Path, rel: &Path, out: &mut Vec<(String, String)>) -> Result<(), SeerError> {
    let entries = fs::read_dir(abs).map_err(|e| io_err(abs, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_err(abs, e))?;
        let ft = entry.file_type().map_err(|e| io_err(&entry.path(), e))?;
        // file_type does not follow the link; metadata/is_dir would.
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if excluded_component(name) {
            continue;
        }
        let child_rel: PathBuf = rel.join(name);
        let child_abs = entry.path();
        if ft.is_dir() {
            walk(&child_abs, &child_rel, out)?;
        } else if ft.is_file() && name.ends_with(".rs") {
            let posix = posix_rel(&child_rel);
            let bytes = fs::read(&child_abs).map_err(|e| io_err(&child_abs, e))?;
            let src = String::from_utf8(bytes)
                .map_err(|_| SeerError::Io(format!("invalid utf-8: {posix}")))?;
            out.push((posix, src));
        }
    }
    Ok(())
}

fn excluded_component(name: &str) -> bool {
    name == "target" || name == ".git" || name == "node_modules" || name.starts_with('.')
}

fn posix_rel(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

fn io_err(path: &Path, err: std::io::Error) -> SeerError {
    SeerError::Io(format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp_tree() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn includes_rs_sorts_posix_and_excludes() {
        let dir = tmp_tree();
        let root = dir.path();
        write(&root.join("b.rs"), "fn b() {}");
        write(&root.join("a.rs"), "fn a() {}");
        write(&root.join("sub/c.rs"), "fn c() {}");
        write(&root.join("A.RS"), "fn upper() {}");
        write(&root.join("target/hidden.rs"), "fn t() {}");
        write(&root.join(".git/hidden.rs"), "fn g() {}");
        write(&root.join("node_modules/hidden.rs"), "fn n() {}");
        write(&root.join(".hidden/x.rs"), "fn h() {}");
        write(&root.join(".dot.rs"), "fn d() {}");
        write(&root.join("sub/.secret.rs"), "fn s() {}");
        write(&root.join("keep.txt"), "not rust");

        let files = collect_rs_files(root).unwrap();
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["a.rs", "b.rs", "sub/c.rs"]);
    }

    #[test]
    fn empty_dir_is_empty() {
        let dir = tmp_tree();
        assert!(collect_rs_files(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn invalid_utf8_is_io() {
        let dir = tmp_tree();
        let path = dir.path().join("bad.rs");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(&[0xff, 0xfe]).unwrap();
        match collect_rs_files(dir.path()) {
            Err(SeerError::Io(msg)) => assert_eq!(msg, "invalid utf-8: bad.rs"),
            other => panic!("expected invalid utf-8, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tmp_tree();
        let root = dir.path();
        write(&root.join("real.rs"), "fn real() {}");
        write(&root.join("other/x.rs"), "fn x() {}");
        symlink(root.join("real.rs"), root.join("link.rs")).unwrap();
        symlink(root.join("other"), root.join("linkdir")).unwrap();

        let files = collect_rs_files(root).unwrap();
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["other/x.rs", "real.rs"]);
    }
}
