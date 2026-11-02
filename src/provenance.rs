//! Hash-chain provenance log: every FUSE operation produces a record chained
//! via SHA-256, so the mount's history can be audited by re-walking the chain.
//!
//! Record hash layout (documented contract, `||` is concatenation):
//! `hash = SHA256(prev_hash[32] || operation || 0x00 || path || 0x00 || status || 0x00 || size_le64)`
//!
//! The genesis `prev_hash` is `SHA256("genesis")`. The chain is a ring buffer:
//! when the capacity is reached the oldest records are dropped (the chain never
//! breaks, it just loses its tail).

use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// Size of a SHA-256 hash in bytes.
pub const HASH_SIZE: usize = 32;

/// One audited FUSE operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub seq: u64,
    pub unix_ts: u64,
    pub operation: String,
    pub path: String,
    pub status: String,
    pub size: u64,
    pub prev_hash: [u8; HASH_SIZE],
    pub hash: [u8; HASH_SIZE],
}

fn genesis_hash() -> [u8; HASH_SIZE] {
    let mut h = Sha256::new();
    h.update(b"genesis");
    h.finalize().into()
}

fn compute_hash(
    prev: &[u8; HASH_SIZE],
    operation: &str,
    path: &str,
    status: &str,
    size: u64,
) -> [u8; HASH_SIZE] {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(operation.as_bytes());
    h.update([0x00]);
    h.update(path.as_bytes());
    h.update([0x00]);
    h.update(status.as_bytes());
    h.update([0x00]);
    h.update(size.to_le_bytes());
    h.finalize().into()
}

/// The append-only (ring) chain of provenance records.
pub struct Chain {
    entries: VecDeque<Entry>,
    capacity: usize,
    count: u64,
    last_hash: [u8; HASH_SIZE],
}

impl Chain {
    /// A new chain whose head is `SHA256("genesis")`.
    pub fn new(capacity: usize) -> Chain {
        Chain {
            entries: VecDeque::with_capacity(capacity.min(4096)),
            capacity: capacity.max(1),
            count: 0,
            last_hash: genesis_hash(),
        }
    }

    /// Append one record; returns its sequence number.
    pub fn log(&mut self, operation: &str, path: &str, status: &str, size: u64) -> u64 {
        let prev = self.last_hash;
        let hash = compute_hash(&prev, operation, path, status, size);
        let entry = Entry {
            seq: self.count,
            unix_ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            operation: operation.to_string(),
            path: path.to_string(),
            status: status.to_string(),
            size,
            prev_hash: prev,
            hash,
        };
        self.last_hash = hash;
        self.count += 1;
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        self.count - 1
    }

    /// Number of records ever logged (not the ring size).
    pub fn total(&self) -> u64 {
        self.count
    }

    /// Records currently retained.
    pub fn retained(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Verify the retained chain: every record's hash must match its contents
    /// and link to the previous retained record. After ring eviction the oldest
    /// retained record's `prev_hash` is treated as the trusted root, so the
    /// surviving suffix always verifies. `Ok(())` or the first bad sequence number.
    pub fn verify(&self) -> Result<(), u64> {
        let mut prev: Option<[u8; HASH_SIZE]> = None;
        for e in &self.entries {
            if let Some(p) = prev {
                if e.prev_hash != p {
                    return Err(e.seq);
                }
            }
            if e.hash != compute_hash(&e.prev_hash, &e.operation, &e.path, &e.status, e.size) {
                return Err(e.seq);
            }
            prev = Some(e.hash);
        }
        Ok(())
    }

    /// Hex of the current head hash (for banners / audits).
    pub fn head_hex(&self) -> String {
        self.last_hash.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_is_verifiable() {
        let mut c = Chain::new(100);
        c.log("getattr", "/session", "ok", 0);
        c.log("read", "/session/org.a/introspect", "ok", 128);
        c.log("write", "/session/org.a/methods", "ok", 13);
        assert_eq!(c.total(), 3);
        assert_eq!(c.verify(), Ok(()));
    }

    #[test]
    fn tampering_is_detected() {
        let mut c = Chain::new(100);
        c.log("getattr", "/", "ok", 0);
        c.log("read", "/x", "ok", 5);
        c.log("read", "/y", "ok", 7);
        // mutate a retained record in place, as a tamperer would
        c.entries[1].size = 999;
        assert_eq!(c.verify(), Err(1));
    }

    #[test]
    fn genesis_is_sha256_of_genesis() {
        let mut c = Chain::new(10);
        c.log("getattr", "/", "ok", 0);
        let e = c.retained().next().unwrap();
        let mut h = Sha256::new();
        h.update(b"genesis");
        let g: [u8; 32] = h.finalize().into();
        assert_eq!(e.prev_hash, g);
    }

    #[test]
    fn ring_never_breaks() {
        let mut c = Chain::new(4);
        for i in 0..20 {
            c.log("op", &format!("/p{i}"), "ok", i);
        }
        assert_eq!(c.total(), 20);
        assert_eq!(c.retained().count(), 4);
        // retained suffix must still form a valid chain
        assert_eq!(c.verify(), Ok(()));
        assert_eq!(c.retained().next().unwrap().seq, 16);
    }
}
