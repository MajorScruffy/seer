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
seer diff REV1 REV2      # outline-diff two git revisions
seer diff-trees a b      # diff two outline text files
```

Exit 0 on success, including a printed diff. Empty stdout means no outline change. Exit 2 usage, 3 runtime.
