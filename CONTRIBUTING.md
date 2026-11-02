# Contributing to stint-busfs

Short rules for a focused codebase.

## Ground truth

- **Demo mode is a feature.** Every operation must degrade cleanly when no bus
  is reachable — no panics, no hangs, honest `demo (no bus)` contents.
- **Contracts and provenance are the product.** New operations must log to the
  provenance chain and run their pre/post checks.
- The virtual tree is a documented surface: `/ <bus> / <service> / <file>`.
  Changing it is breaking — say so in the PR.

## Dev environment

A stable Rust toolchain (1.80+) is everything. Tests need neither a D-Bus
daemon nor `/dev/fuse`:

```bash
cargo test
```

If you have a session bus and want to try the real thing:

```bash
cargo run -- mount /tmp/busfs
ls /tmp/busfs/session
```

## Before you open a PR

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] New behaviour has a test (the core is deliberately mount-free to keep this cheap)
- [ ] Demo mode still works end-to-end
- [ ] README/README.ru.md updated if the tree, CLI or docs changed

## PR style

Small, single-purpose PRs with a conventional-commit title (`feat:`, `fix:`,
`docs:`, `ci:`, `refactor:`). The PR template asks for the type, the bus/mount
behaviour impact and the checks — fill it in and you are done.
