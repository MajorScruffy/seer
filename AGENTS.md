When reviewing rust, java, or typescript control flow — or a git diff of behavior — run `seer` first. Prefer `seer -- path` over a whole-repo dump. Read `seer --help`. Empty stdout means no outline change; a printed diff is success (exit 0). Tree lines are `path:line` jump targets.

Using seer: [skills/seer/SKILL.md](skills/seer/SKILL.md)
Changing seer: [docs/spec.md](docs/spec.md). Call graph: [docs/graph.md](docs/graph.md). `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` are done.
