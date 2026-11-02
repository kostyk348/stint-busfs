# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.0] — 2026-11-02

### Added — Rust port (replaces the C implementation)

- Single Rust crate (`busfs` lib + `stint-busfs` bin). Semantics of the C
  prototype preserved; internals upgraded:
  - FUSE via `fuser` with a pure-Rust mount implementation — no libfuse link
    dependency; D-Bus via pure-Rust `zbus` — no `libdbus-1`, no OpenSSL.
  - Live service discovery through `ListNames` (well-known names only, static
    list kept as demo-mode fallback).
  - The `properties` virtual file now serves a real `Properties.GetAll`
    (sorted `key = value`) instead of an introspection-XML placeholder; new
    `interfaces` and `signals` files parse live introspection.
  - Stable inode model with a real `lookup` (the prototype relied on the
    hand-rolled op table), so path resolution works kernel-side.
  - DSO contracts are now enforced: pre-conditions DENY (size bounds, path
    checks), post-conditions FALLBACK, violations counted and logged.
  - SHA-256 provenance chain with a documented record layout, ring retention
    and a `verify()` that survives ring eviction; tamper detection is tested.
- `stint-busfs list [--system]` — inspect a bus without mounting.
- 18 tests covering the virtual tree, name rules, contract escalation,
  provenance (incl. tamper detection) and demo mode; CI (fmt, clippy, test,
  release build, CLI smoke) needs neither a bus nor `/dev/fuse`.
- README in English and Russian, CONTRIBUTING, changelog, PR/issue templates.

### Changed

- CLI: `stint-busfs <mountpoint>` → `stint-busfs mount <mountpoint>`; help and
  version flags now come from clap.
- Per-call D-Bus timeout is zbus's default (25 s) instead of the prototype's 5 s.

### Removed

- The C sources (`src/*.c`, `include/*.h`) and the Makefile — fully replaced by
  the Rust crate.

[0.2.0]: https://github.com/kostyk348/stint-busfs/releases/tag/v0.2.0
