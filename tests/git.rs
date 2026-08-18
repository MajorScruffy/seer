use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["-c", "init.defaultBranch=main", "init"]);
    git(dir.path(), &["config", "user.email", "seer@example.com"]);
    git(dir.path(), &["config", "user.name", "seer-test"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn seer(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_seer"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run seer")
}

const SRC_CLEAN: &str = "\
fn main() {
    return;
}
";

const SRC_DIRTY: &str = "\
fn main() {
    if true {
        return;
    }
}
";

const DIRTY_VS_HEAD: &str = "\
--- HEAD
+++ WORKTREE
@@ -1,2 +1,3 @@
 fn main
-  return
+  if true
+    return
";

const TWO_REVS: &str = "\
--- HEAD~1
+++ HEAD
@@ -1,2 +1,3 @@
 fn main
-  return
+  if true
+    return
";

const UNTRACKED: &str = "\
--- HEAD
+++ WORKTREE
@@ -1,2 +1,5 @@
 fn main
   return
+
+fn extra
+  return
";

#[test]
fn git_dirty_vs_head() {
    let repo = init_repo();
    let root = repo.path();
    write(&root.join("src.rs"), SRC_CLEAN);
    git(root, &["add", "src.rs"]);
    git(root, &["commit", "-m", "c1"]);
    write(&root.join("src.rs"), SRC_DIRTY);

    let no_args = seer(root, &[]);
    assert_eq!(
        no_args.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&no_args.stderr)
    );
    assert_eq!(no_args.stdout, DIRTY_VS_HEAD.as_bytes());

    let diff = seer(root, &["diff"]);
    assert_eq!(
        diff.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&diff.stderr)
    );
    assert_eq!(diff.stdout, DIRTY_VS_HEAD.as_bytes());

    git(root, &["add", "src.rs"]);
    git(root, &["commit", "-m", "c2"]);
    let clean = seer(root, &[]);
    assert_eq!(
        clean.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(clean.stdout.is_empty());
}

#[test]
fn git_diff_two_revs() {
    let repo = init_repo();
    let root = repo.path();
    write(&root.join("src.rs"), SRC_CLEAN);
    git(root, &["add", "src.rs"]);
    git(root, &["commit", "-m", "c1"]);
    write(&root.join("src.rs"), SRC_DIRTY);
    git(root, &["add", "src.rs"]);
    git(root, &["commit", "-m", "c2"]);

    let out = seer(root, &["diff", "HEAD~1", "HEAD"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, TWO_REVS.as_bytes());
}

#[test]
fn git_not_a_repo() {
    let dir = tempfile::tempdir().unwrap();
    let out = seer(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not a git repository"), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn git_untracked_file() {
    let repo = init_repo();
    let root = repo.path();
    write(&root.join("a.rs"), SRC_CLEAN);
    git(root, &["add", "a.rs"]);
    git(root, &["commit", "-m", "c1"]);
    write(
        &root.join("b.rs"),
        "\
fn extra() {
    return;
}
",
    );

    let out = seer(root, &[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, UNTRACKED.as_bytes());
}

#[test]
fn git_from_subdir() {
    let repo = init_repo();
    let root = repo.path();
    write(
        &root.join("root.rs"),
        "\
fn root() {
    return;
}
",
    );
    write(&root.join("src/main.rs"), SRC_CLEAN);
    git(root, &["add", "root.rs", "src/main.rs"]);
    git(root, &["commit", "-m", "c1"]);

    let from_root = seer(root, &[]);
    assert_eq!(
        from_root.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&from_root.stderr)
    );
    assert!(from_root.stdout.is_empty());

    let src_dir = root.join("src");
    let from_src = seer(&src_dir, &[]);
    assert_eq!(
        from_src.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&from_src.stderr)
    );
    assert!(from_src.stdout.is_empty());
    assert_eq!(from_root.stdout, from_src.stdout);

    write(&root.join("src/main.rs"), SRC_DIRTY);
    let dirty_root = seer(root, &[]);
    let dirty_src = seer(&src_dir, &[]);
    assert_eq!(
        dirty_root.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&dirty_root.stderr)
    );
    assert_eq!(dirty_src.status.code(), Some(1));
    assert_eq!(dirty_root.stdout, dirty_src.stdout);
}

#[test]
fn git_diff_one_rev() {
    let repo = init_repo();
    let root = repo.path();
    write(&root.join("src.rs"), SRC_CLEAN);
    git(root, &["add", "src.rs"]);
    git(root, &["commit", "-m", "c1"]);
    write(&root.join("src.rs"), SRC_DIRTY);

    let no_args = seer(root, &[]);
    let one_rev = seer(root, &["diff", "HEAD"]);
    assert_eq!(
        no_args.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&no_args.stderr)
    );
    assert_eq!(one_rev.status.code(), Some(1));
    assert_eq!(no_args.stdout, one_rev.stdout);
    assert_eq!(one_rev.stdout, DIRTY_VS_HEAD.as_bytes());
}

#[test]
fn git_invalid_rev() {
    let repo = init_repo();
    let out = seer(repo.path(), &["diff", "not-a-rev"]);
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("invalid revision"), "{err}");
    assert!(out.stdout.is_empty());
}
