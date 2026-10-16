# stint-busfs

[![Release](https://img.shields.io/github/v/release/kostyk348/stint-busfs?sort=semver&color=blue)](https://github.com/kostyk348/stint-busfs/releases)
[![License](https://img.shields.io/badge/license-Apache-2.0-green)](https://github.com/kostyk348/stint-busfs/blob/master/LICENSE)
[![Last commit](https://img.shields.io/github/last-commit/kostyk348/stint-busfs?branch=master)](https://github.com/kostyk348/stint-busfs/commits/master)

**D-Bus as a Virtual Filesystem** — mount D-Bus at `/bus/` and poke it with `ls`, `cat`, `echo`.

```
ls /bus/session
cat /bus/session/org.freedesktop.DBus/introspect
echo "ListNames" > /bus/session/org.freedesktop.DBus/methods
```

## Architecture

```
┌─ fuse_loop_mt() ───────┐   ┌─ D-Bus dispatch thread ─┐
│ ls | cat | echo  → FUSE │   │  dbus_connection_read()  │
│           ↓             │   │  (signals, events)       │
│     busfs_readdir()     │   └──────────────────────────┘
│     busfs_read()        │
│     busfs_write()       │
│           ↓             │
│   dbus_connection_*()   │
└─────────────────────────┘
```

- **FUSE3** multi-threaded (`fuse_new` + `fuse_loop_mt`)
- **Real D-Bus** calls via `libdbus-1` (Introspect, GetAll, method calls)
- **DSO contracts** on every operation (precondition/postcondition/invariant)
- **SHA-256 hash-chain provenance** — every FUSE op logged with hash chaining

## Build

```bash
make
```

Dependencies: `fuse3`, `dbus-1`, `openssl`.

## Usage

```bash
./stint-busfs /tmp/busfs
# in another terminal:

ls /tmp/busfs/session
cat /tmp/busfs/session/org.freedesktop.DBus/introspect
```

## Path structure

```
/
├── session/          # session bus services
│   ├── org.freedesktop.DBus/
│   │   ├── introspect    → introspection XML
│   │   ├── properties    → property values
│   │   ├── methods       → write here to call methods
│   │   ├── interfaces
│   │   └── signals
│   ├── org.freedesktop.NetworkManager/
│   ├── org.freedesktop.UPower/
│   └── ...
└── system/           # system bus services (not yet)
```

## Why

D-Bus is powerful but invisible from the shell. `stint-busfs` makes every
D-Bus service look like a directory tree — shell-accessible, pipeable,
scriptable.

Part of the **Proximated Linux** vision: redesign Linux IPC through
DSO contract principles.
