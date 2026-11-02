<div align="center">

# stint-busfs

### D-Bus as a Virtual Filesystem

**Mount a D-Bus bus at a mountpoint and poke it with `ls`, `cat`, `echo` —
shell-accessible, pipeable, scriptable, with DSO contracts and a SHA-256
provenance chain on every operation.**

[![Release](https://img.shields.io/github/v/release/kostyk348/stint-busfs?sort=semver&color=0ea5e9&label=release)](https://github.com/kostyk348/stint-busfs/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/kostyk348/stint-busfs/ci.yml?branch=master&label=CI)](https://github.com/kostyk348/stint-busfs/actions/workflows/ci.yml)
[![tests](https://img.shields.io/badge/tests-18%20passing-22c55e)](#development)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)](https://www.rust-lang.org)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](CONTRIBUTING.md)

[English](README.md) · [Русский](README.ru.md)

</div>

---

## What is stint-busfs?

D-Bus is powerful but invisible from the shell. `stint-busfs` mounts a bus as a
filesystem, so every well-known service becomes a directory and every operation
becomes a file:

```bash
stint-busfs mount /tmp/busfs
# in another terminal:
ls /tmp/busfs/session
cat /tmp/busfs/session/org.freedesktop.DBus/introspect
echo "ListNames" > /tmp/busfs/session/org.freedesktop.DBus/methods
```

Part of the **Proximated Linux** vision: redesign Linux IPC through DSO
contract principles.

## The virtual tree

```
/<bus>/<service>/<file>          e.g. /session/org.freedesktop.UPower/properties
```

| file | what `cat` gives | what writing does |
|---|---|---|
| `introspect` | D-Bus introspection XML | — |
| `properties` | `Properties.GetAll`, formatted `key = value` | — |
| `interfaces` | interface names discovered from introspection | — |
| `signals` | signal names discovered from introspection | — |
| `methods` | usage hint | `echo "MethodName" > methods` calls it |

No bus reachable? The mount still works in **demo mode**: the same tree with a
static service list and honest `demo (no bus)` file contents.

## Architecture

```
┌─ fuser (FUSE3, multi-threaded) ─┐   ┌─ zbus bridge (pure Rust) ─┐
│ ls | cat | echo   → FUSE ops    │   │  Introspect / GetAll /    │
│        ↓                        │   │  ListNames / method calls │
│  vtree (path → node, inodes)    │   └───────────────────────────┘
│        ↓                        │
│  contracts + provenance chain   │
└─────────────────────────────────┘
```

- **FUSE3 multi-threaded** via `fuser` — no libfuse link-time dependency, the
  mount implementation is pure Rust (mounts through `fusermount3`).
- **Real D-Bus** via `zbus` — a pure-Rust implementation, no `libdbus-1`, no OpenSSL.
- **DSO contracts** on every operation: pre-conditions DENY, post-conditions FALLBACK, violations counted and logged.
- **SHA-256 hash-chain provenance** — one record per FUSE operation, verifiable by re-walking the chain; tampering is detectable.

## What the Rust port changed (v0.2)

The prototype was C on libfuse3 + libdbus + OpenSSL. The port keeps the
semantics and upgrades the internals:

| | C prototype | Rust 0.2 |
|---|---|---|
| FUSE | libfuse3, hand-rolled op table | `fuser`, pure-Rust mount |
| D-Bus | libdbus-1 C API | `zbus` (pure Rust) |
| hashing | OpenSSL `SHA256` | `sha2` crate |
| service list | 7 hardcoded names | live `ListNames`, fallback static list |
| `properties` file | placeholder (introspection XML) | real `Properties.GetAll`, sorted `key = value` |
| `interfaces`/`signals` files | — | parsed from live introspection |
| contracts | stubs (always pass) | enforced size bounds, path checks, violation counter |
| inode model | implicit | stable lookup/getattr/readdir inodes |
| tests | none | 18 (tree, contracts, provenance incl. tamper detection, demo mode) |

## Install

```bash
cargo install --path .
```

Runtime requirements: a Linux kernel with FUSE (`fusermount3` present on every
distro with `fuse3` installed). No C libraries to build or link.

## Usage

```bash
stint-busfs mount <mountpoint> [--system] [--no-contracts] [--no-provenance]
stint-busfs list [--system]          # services on a bus, no mounting
stint-busfs --version
```

## Development

```bash
cargo fmt --check                          # formatting
cargo clippy --all-targets -- -D warnings  # lints, zero tolerance
cargo test                                 # 18 tests, no bus or mount needed
cargo build --release
```

Everything except the thin FUSE glue is testable offline: path parsing, the
virtual tree, contract escalation and the provenance chain (including tamper
detection) run in plain unit tests; CI needs neither a bus nor `/dev/fuse`.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [CHANGELOG.md](CHANGELOG.md).

## Caveats

* Method calls currently pass no arguments (as in the prototype); a typed
  argument syntax on the `methods` file is the next step.
* `properties` uses the first interface from introspection; multi-interface
  property dumps are future work.
* The service tree lists well-known names only — unique `:1.x` connections are
  filtered out on purpose.
