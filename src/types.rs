//! Configuration and bus selection.

/// Which bus to mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusType {
    /// The per-user session bus (default).
    Session,
    /// The system-wide bus.
    System,
}

impl BusType {
    /// Directory name under the mountpoint.
    pub fn dir_name(self) -> &'static str {
        match self {
            BusType::Session => "session",
            BusType::System => "system",
        }
    }

    /// Human-readable name used in banners and provenance records.
    pub fn label(self) -> &'static str {
        match self {
            BusType::Session => "session",
            BusType::System => "system",
        }
    }
}

/// Runtime configuration (CLI flags land here).
#[derive(Clone, Debug)]
pub struct Config {
    pub bus_type: BusType,
    pub enable_provenance: bool,
    pub enable_contracts: bool,
    pub enable_signals: bool,
    pub max_read_size: usize,
    pub max_write_size: usize,
    pub contract_timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bus_type: BusType::Session,
            enable_provenance: true,
            enable_contracts: true,
            enable_signals: true,
            max_read_size: 64 * 1024,
            max_write_size: 64 * 1024,
            contract_timeout_ms: 5000,
        }
    }
}
