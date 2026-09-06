# seer

Control-and-call outlines of source code.

Behavior is specified in [docs/spec.md](docs/spec.md).

## Install

```sh
cargo install --path .
```

## Usage

```sh
seer <file.rs|java|ts>   # tree (path:line on fns and expanded calls)
seer                     # outline-diff dirty worktree vs HEAD
seer graph PATH          # call graph (Mermaid)
seer graph --format html PATH > graph.html
seer diff REV1 REV2      # outline-diff two git revisions
seer diff-trees a b      # diff two outline text files
```

Exit 0 on success, including a printed diff. Empty stdout means no outline change. Exit 2 usage, 3 runtime.

`seer --help` prints a sample tree. `seer-view` is a lazygit-style TUI of `seer diff`: commits, files, outline. `d` toggles inline vs side-by-side. One commit vs its parent; `J`/`K` selects a range (first..last). `q` quits.
