---
name: seer
description: >
  Control-and-call outlines and outline-diffs of rust, java, and typescript.
  Use when reviewing a git diff, tracing what a function does, or comparing
  revisions without reading whole files.
---

# seer

Sparse control-and-call outline of source, then a unified diff of those outlines. Not an AST dump. Local callees expand in place; logging is omitted.

Read `seer --help` for argv, exits, and a sample tree. This file is when to run it.

## Steps

1. Outline a file: `seer src/foo.rs`. Outline-diff one path vs HEAD: `seer -- src/foo.rs` or `seer diff -- src/foo.rs`. Done when stdout is the tree/diff (or empty).
2. Whole worktree vs HEAD: `seer`. Two commits: `seer diff REV1 REV2`. Path-limit either with `-- path`. Empty stdout = no outline change. Exit 0 even when a diff is printed; do not retry.
3. Open `path:line` from tree lines (`src/foo.rs:12 fn process`, `src/bar.rs:3 handle()`). Diff lines carry `path` without the line so an edit that only shifts lines stays quiet.
4. If stdout floods the turn, rerun with a narrower path or `--max-lines N`. Done when the kept lines plus the truncation notice fit.

Do not conclude what the control flow does until seer stdout (or empty) is in context.
