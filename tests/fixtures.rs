use std::fs;
use std::path::{Path, PathBuf};

use seer::collect::collect_rs_files;
use seer::{diff_text, outline_files};

#[test]
fn tree_goldens() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree");
    let mut names: Vec<String> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read {}: {e}", root.display()))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(entry.file_name().to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "no tree fixtures under {}",
        root.display()
    );
    for name in names {
        let dir = root.join(&name);
        let actual = outline_tree_fixture(&dir);
        let expected = fs::read(dir.join("expected.txt"))
            .unwrap_or_else(|e| panic!("read expected.txt in {}: {e}", dir.display()));
        assert_bytes_eq(&name, &expected, actual.as_bytes());
    }
}

#[test]
fn diff_goldens() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diff");
    let mut names: Vec<String> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read {}: {e}", root.display()))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(entry.file_name().to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "no diff fixtures under {}",
        root.display()
    );
    for name in names {
        let dir = root.join(&name);
        let a = fs::read_to_string(dir.join("a.txt"))
            .unwrap_or_else(|e| panic!("read a.txt in {}: {e}", dir.display()));
        let b = fs::read_to_string(dir.join("b.txt"))
            .unwrap_or_else(|e| panic!("read b.txt in {}: {e}", dir.display()));
        let expected = fs::read(dir.join("expected.txt"))
            .unwrap_or_else(|e| panic!("read expected.txt in {}: {e}", dir.display()));
        let actual = diff_text(&a, &b, "a", "b");
        assert_bytes_eq(&format!("diff/{name}"), &expected, actual.as_bytes());
    }
}

fn outline_tree_fixture(dir: &Path) -> String {
    let input_dir = dir.join("input");
    for name in ["input.rs", "input.java", "input.ts", "input.tsx"] {
        let path = dir.join(name);
        if path.is_file() {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            return outline_files(&[(name.to_string(), contents)]);
        }
    }
    if input_dir.is_dir() {
        let files = collect_rs_files(&input_dir)
            .unwrap_or_else(|e| panic!("collect {}: {e}", input_dir.display()));
        outline_files(&files)
    } else {
        panic!(
            "fixture {} has neither input.rs/java/ts/tsx nor input/",
            dir.display()
        );
    }
}

fn assert_bytes_eq(name: &str, expected: &[u8], actual: &[u8]) {
    if expected == actual {
        return;
    }
    let exp = String::from_utf8_lossy(expected);
    let act = String::from_utf8_lossy(actual);
    panic!(
        "{name} mismatch (expected {} bytes, actual {} bytes)\n{}",
        expected.len(),
        actual.len(),
        unified_diff(&exp, &act)
    );
}

fn unified_diff(expected: &str, actual: &str) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    let ops = diff_ops(&a, &b);
    let mut out = String::from("--- expected\n+++ actual\n");
    let (old_start, old_len) = if a.is_empty() { (0, 0) } else { (1, a.len()) };
    let (new_start, new_len) = if b.is_empty() { (0, 0) } else { (1, b.len()) };
    out.push_str(&format!(
        "@@ -{old_start},{old_len} +{new_start},{new_len} @@\n"
    ));
    for op in ops {
        match op {
            DiffOp::Keep(s) => {
                out.push(' ');
                out.push_str(s);
                out.push('\n');
            }
            DiffOp::Del(s) => {
                out.push('-');
                out.push_str(s);
                out.push('\n');
            }
            DiffOp::Add(s) => {
                out.push('+');
                out.push_str(s);
                out.push('\n');
            }
        }
    }
    out
}

enum DiffOp<'a> {
    Keep(&'a str),
    Del(&'a str),
    Add(&'a str),
}

fn diff_ops<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<DiffOp<'a>> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..n {
        for j in 0..m {
            dp[i + 1][j + 1] = if a[i] == b[j] {
                dp[i][j] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let mut i = n;
    let mut j = m;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            ops.push(DiffOp::Keep(a[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(DiffOp::Add(b[j - 1]));
            j -= 1;
        } else {
            ops.push(DiffOp::Del(a[i - 1]));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}
