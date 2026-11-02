//! stint-busfs — D-Bus as a Virtual Filesystem.
//!
//! Async architecture: `fuser::mount2` runs the multi-threaded FUSE loop; each
//! handler makes synchronous D-Bus calls (blocking per thread) through the
//! pure-Rust zbus bridge. DSO contracts + SHA-256 hash-chain provenance on
//! every operation.

use busfs::{bridge, fuse_fs, types};
use clap::{Parser, Subcommand};
use fuser::MountOption;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "stint-busfs",
    version,
    about = "D-Bus as a Virtual Filesystem — mount a bus and poke it with ls, cat, echo"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Mount a bus at a mountpoint (default)
    Mount {
        /// Where to mount the bus
        mountpoint: PathBuf,
        /// Connect to the system bus (default: session)
        #[arg(long)]
        system: bool,
        /// Disable DSO contract checks
        #[arg(long)]
        no_contracts: bool,
        /// Disable the SHA-256 provenance chain
        #[arg(long)]
        no_provenance: bool,
    },
    /// Print services visible on a bus without mounting
    List {
        /// Connect to the system bus (default: session)
        #[arg(long)]
        system: bool,
    },
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let cli = Cli::parse();
    match cli.cmd {
        Command::List { system } => {
            let bus_type = if system {
                types::BusType::System
            } else {
                types::BusType::Session
            };
            println!("stint-busfs — services on the {0} bus:", bus_type.label());
            match bridge::Bridge::connect(bus_type) {
                Some(b) => {
                    let names = b.list_services();
                    if names.is_empty() {
                        println!("  (no well-known names found)");
                    }
                    for n in names {
                        println!("  {n}");
                    }
                    0
                }
                None => {
                    eprintln!("  D-Bus {0} bus: NOT AVAILABLE", bus_type.label());
                    1
                }
            }
        }
        Command::Mount {
            mountpoint,
            system,
            no_contracts,
            no_provenance,
        } => mount(mountpoint, system, no_contracts, no_provenance),
    }
}

fn mount(mountpoint: PathBuf, system: bool, no_contracts: bool, no_provenance: bool) -> i32 {
    println!("stint-busfs — D-Bus as Virtual Filesystem");
    println!("  DSO contracts + SHA-256 hash-chain provenance\n");

    let bus_type = if system {
        types::BusType::System
    } else {
        types::BusType::Session
    };
    let config = types::Config {
        bus_type,
        enable_contracts: !no_contracts,
        enable_provenance: !no_provenance,
        ..types::Config::default()
    };

    let bus_bridge = bridge::Bridge::connect(bus_type);
    match &bus_bridge {
        Some(_) => println!("  D-Bus {} bus: CONNECTED", bus_type.label()),
        None => eprintln!("  D-Bus: NOT AVAILABLE — running in demo mode"),
    }

    let fs = fuse_fs::BusFs::new(config, bus_bridge);
    println!(
        "  Provenance chain: {}",
        if fs.provenance.is_some() {
            "initialized (capacity=1000)"
        } else {
            "disabled"
        }
    );
    println!(
        "  DSO contracts: {}",
        if fs.config.enable_contracts {
            "ENABLED"
        } else {
            "disabled"
        }
    );

    let dir = fs.config.bus_type.dir_name();
    println!("\n  Mount point: {}", mountpoint.display());
    println!("  Try:  ls {0}/{1}", mountpoint.display(), dir);
    println!(
        "  Try:  cat {0}/{1}/org.freedesktop.UPower/introspect",
        mountpoint.display(),
        dir
    );
    println!(
        "  Try:  cat {0}/{1}/org.freedesktop.DBus/properties",
        mountpoint.display(),
        dir
    );
    println!("\n  PID: {}  Hit Ctrl-C to unmount", std::process::id());
    println!("──────────────────────────────────────────────────────");

    let options = [
        MountOption::FSName("stint-busfs".into()),
        MountOption::DefaultPermissions,
    ];
    match fuser::mount2(fs, &mountpoint, &options) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("stint-busfs: mount failed: {e}");
            1
        }
    }
}
