//! stint-busfs core — D-Bus as a Virtual Filesystem.
//!
//! The crate is structured so that everything except the thin FUSE glue
//! ([`fuse_fs`]) is testable without a mounted filesystem or even a running bus:
//!
//! - [`vtree`] — the virtual tree: path parsing, node kinds, directory listings
//! - [`bridge`] — the D-Bus bridge (pure-Rust `zbus`), with an offline demo fallback
//! - [`provenance`] — SHA-256 hash-chain audit log, one record per FUSE operation
//! - [`contract`] — DSO contract validation (pre/post conditions) with escalation
//!
//! Mount side (bin `stint-busfs`): `fuser` + this lib.

pub mod bridge;
pub mod contract;
pub mod fuse_fs;
pub mod provenance;
pub mod types;
pub mod vtree;

pub use types::{BusType, Config};
pub use vtree::{FileKind, VPath};
