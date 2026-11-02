//! The virtual tree: parsing mount paths into bus / service / file parts.
//!
//! Path layout (all pure logic, no I/O — everything here is unit-testable):
//!
//! ```text
//! /<bus>/<service>/<file>     e.g. /session/org.freedesktop.UPower/properties
//! ```

use crate::types::BusType;

/// The virtual "file" exposed inside every service directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// D-Bus introspection XML.
    Introspect,
    /// Property values (org.freedesktop.DBus.Properties.GetAll).
    Properties,
    /// Interface names discovered from introspection.
    Interfaces,
    /// Signal names discovered from introspection.
    Signals,
    /// Write here to call a method: `echo "MethodName args" > methods`.
    Methods,
}

impl FileKind {
    /// Parse a file name.
    pub fn from_name(name: &str) -> Option<FileKind> {
        match name {
            "introspect" => Some(FileKind::Introspect),
            "properties" => Some(FileKind::Properties),
            "interfaces" => Some(FileKind::Interfaces),
            "signals" => Some(FileKind::Signals),
            "methods" => Some(FileKind::Methods),
            _ => None,
        }
    }

    /// Canonical file name.
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Introspect => "introspect",
            FileKind::Properties => "properties",
            FileKind::Interfaces => "interfaces",
            FileKind::Signals => "signals",
            FileKind::Methods => "methods",
        }
    }
}

/// All virtual files a service directory exposes.
pub const SERVICE_FILES: [FileKind; 5] = [
    FileKind::Properties,
    FileKind::Interfaces,
    FileKind::Methods,
    FileKind::Signals,
    FileKind::Introspect,
];

/// A parsed mount path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VPath {
    /// `/` — the mount root.
    Root,
    /// `/<bus>` — session or system.
    Bus(BusType),
    /// `/<bus>/<service>` — a service directory.
    Service(BusType, String),
    /// `/<bus>/<service>/<file>` — a virtual file.
    File(BusType, String, FileKind),
    /// Anything else (does not exist).
    Unknown,
}

impl VPath {
    /// Parse an absolute FUSE path.
    pub fn parse(path: &str) -> VPath {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        match parts.as_slice() {
            [] => VPath::Root,
            [bus] => match BusDir::from_name(bus) {
                Some(b) => VPath::Bus(b.0),
                None => VPath::Unknown,
            },
            [bus, service] => match BusDir::from_name(bus) {
                Some(b) if is_service_name(service) => VPath::Service(b.0, (*service).to_string()),
                _ => VPath::Unknown,
            },
            [bus, service, file] => match (BusDir::from_name(bus), FileKind::from_name(file)) {
                (Some(b), Some(f)) if is_service_name(service) => {
                    VPath::File(b.0, (*service).to_string(), f)
                }
                _ => VPath::Unknown,
            },
            _ => VPath::Unknown,
        }
    }

    /// The bus level, if the path reaches one.
    pub fn bus(&self) -> Option<BusType> {
        match self {
            VPath::Bus(b) | VPath::Service(b, _) | VPath::File(b, ..) => Some(*b),
            _ => None,
        }
    }

    /// The service name, if the path reaches one.
    pub fn service(&self) -> Option<&str> {
        match self {
            VPath::Service(_, s) | VPath::File(_, s, ..) => Some(s),
            _ => None,
        }
    }

    /// The file kind, if the path is a virtual file.
    pub fn file(&self) -> Option<FileKind> {
        match self {
            VPath::File(_, _, f) => Some(*f),
            _ => None,
        }
    }

    /// True when the path addresses a directory (root, bus or service).
    pub fn is_dir(&self) -> bool {
        matches!(self, VPath::Root | VPath::Bus(_) | VPath::Service(..))
    }

    /// Directory entries for a directory path (no leading `.`/`..`).
    pub fn entries(&self, services: &[String]) -> Vec<(String, bool)> {
        match self {
            VPath::Root => vec![
                (BusType::Session.dir_name().to_string(), true),
                (BusType::System.dir_name().to_string(), true),
            ],
            VPath::Bus(_) => services.iter().map(|s| (s.clone(), true)).collect(),
            VPath::Service(..) => SERVICE_FILES
                .iter()
                .map(|f| (f.name().to_string(), false))
                .collect(),
            _ => vec![],
        }
    }
}

/// Newtype so `from_name` can be private yet used in the match above.
#[derive(Clone, Copy)]
struct BusDir(BusType);

impl BusDir {
    fn from_name(name: &str) -> Option<BusDir> {
        match name {
            "session" => Some(BusDir(BusType::Session)),
            "system" => Some(BusDir(BusType::System)),
            _ => None,
        }
    }
}

/// D-Bus well-known name check (the spec's rule): dot-separated elements of
/// `[A-Za-z0-9_]`, at least two elements, none empty, none starting with a digit.
pub fn is_service_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(':')
        && name.contains('.')
        && name.split('.').all(|p| {
            let mut it = p.bytes();
            match it.next() {
                Some(f) if f.is_ascii_alphabetic() || f == b'_' => {
                    it.all(|b| b.is_ascii_alphanumeric() || b == b'_')
                }
                _ => false,
            }
        })
}

/// D-Bus convention: service name → object path
/// `org.freedesktop.UPower` → `/org/freedesktop/UPower`.
pub fn service_to_object_path(service: &str) -> String {
    format!("/{}", service.replace('.', "/"))
}

/// The services shown when no live bus is reachable (the prototype's static list).
pub fn fallback_services() -> Vec<String> {
    [
        "org.freedesktop.DBus",
        "org.freedesktop.NetworkManager",
        "org.freedesktop.UPower",
        "org.freedesktop.systemd1",
        "org.freedesktop.login1",
        "org.freedesktop.hostname1",
        "org.freedesktop.timedate1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_full_tree() {
        assert_eq!(VPath::parse("/"), VPath::Root);
        assert_eq!(VPath::parse("/session"), VPath::Bus(BusType::Session));
        assert_eq!(VPath::parse("/system"), VPath::Bus(BusType::System));
        assert_eq!(
            VPath::parse("/session/org.freedesktop.UPower"),
            VPath::Service(BusType::Session, "org.freedesktop.UPower".into())
        );
        assert_eq!(
            VPath::parse("/session/org.freedesktop.UPower/properties"),
            VPath::File(
                BusType::Session,
                "org.freedesktop.UPower".into(),
                FileKind::Properties
            )
        );
        assert_eq!(VPath::parse("/nope"), VPath::Unknown);
        assert_eq!(VPath::parse("/session/Not_A_Service"), VPath::Unknown);
        assert_eq!(VPath::parse("/session/org.a/what"), VPath::Unknown);
        assert_eq!(VPath::parse("/session/a/b/c"), VPath::Unknown);
    }

    #[test]
    fn accessors() {
        let p = VPath::parse("/system/org.freedesktop.login1/signals");
        assert_eq!(p.bus(), Some(BusType::System));
        assert_eq!(p.service(), Some("org.freedesktop.login1"));
        assert_eq!(p.file(), Some(FileKind::Signals));
        assert!(!p.is_dir());
        assert!(VPath::parse("/session/org.a").is_dir());
    }

    #[test]
    fn service_object_paths() {
        assert_eq!(
            service_to_object_path("org.freedesktop.UPower"),
            "/org/freedesktop/UPower"
        );
        assert_eq!(service_to_object_path("org.a"), "/org/a");
    }

    #[test]
    fn service_name_rules() {
        assert!(is_service_name("org.freedesktop.DBus"));
        assert!(is_service_name("org.a_1.B"));
        assert!(!is_service_name(":1.42"), "unique names are filtered");
        assert!(!is_service_name("single"));
        assert!(!is_service_name(""));
        assert!(!is_service_name("org..a"));
        assert!(!is_service_name("9org.a"));
    }

    #[test]
    fn entries_shape() {
        let root = VPath::parse("/").entries(&[]);
        assert_eq!(
            root,
            vec![("session".to_string(), true), ("system".to_string(), true)]
        );
        let bus = VPath::parse("/session").entries(&fallback_services());
        assert_eq!(bus.len(), 7);
        assert_eq!(bus[0], ("org.freedesktop.DBus".to_string(), true));
        let svc = VPath::parse("/session/org.a").entries(&[]);
        assert_eq!(svc.len(), 5);
        assert!(svc.iter().all(|(_, is_dir)| !*is_dir));
        assert!(VPath::parse("/session/org.a/introspect")
            .entries(&[])
            .is_empty());
    }
}
