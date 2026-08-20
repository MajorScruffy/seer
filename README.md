# seer

Control-and-call outlines of source code.

Behavior is specified in [docs/spec.md](docs/spec.md).

## Install

```sh
cargo install --path .
```

## Usage

```sh
seer <file.rs|java|ts>   # tree (outline)
seer diff-trees a b      # diff two outline text files
seer diff REV1 REV2      # outline-diff two git revisions
seer                     # dirty worktree vs HEAD
```
