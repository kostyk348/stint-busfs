//! FUSE glue: maps the virtual tree onto `fuser` operations.
//!
//! Thin by design — path resolution, contents and contracts live in the lib
//! modules and are unit-tested without a mount. Stable inodes are assigned in
//! [`BusFs::refresh`]; `lookup`/`getattr`/`readdir`/`read`/`write` follow the
//! prototype's semantics (dynamic content, direct I/O, provenance on every op).

use crate::bridge::Bridge;
use crate::contract::{validate, Check, Contract, Kind, Verdict};
use crate::provenance::Chain;
use crate::types::Config;
use crate::vtree::{FileKind, VPath, SERVICE_FILES};
use fuser::{
    FileAttr, FileType, Filesystem, KernelConfig, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs, ReplyWrite, Request,
};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(0);
const ROOT_INO: u64 = 1;

/// The mounted filesystem state.
pub struct BusFs {
    pub config: Config,
    pub bridge: Option<Bridge>,
    pub provenance: Option<Mutex<Chain>>,
    services: Vec<String>,
    /// ino -> path
    paths: HashMap<u64, String>,
    /// path -> ino
    inos: HashMap<String, u64>,
    next_service_ino: u64,
}

fn now() -> SystemTime {
    SystemTime::now()
}

impl BusFs {
    /// Build the filesystem; `bridge` is `None` in demo mode.
    pub fn new(config: Config, bridge: Option<Bridge>) -> BusFs {
        let provenance = if config.enable_provenance {
            Some(Mutex::new(Chain::new(1000)))
        } else {
            None
        };
        let mut fs = BusFs {
            config,
            bridge,
            provenance,
            services: Vec::new(),
            paths: HashMap::new(),
            inos: HashMap::new(),
            next_service_ino: 100,
        };
        fs.paths.insert(ROOT_INO, "/".into());
        fs.inos.insert("/".into(), ROOT_INO);
        fs.refresh();
        fs
    }

    /// (Re)assign inodes for the current service list.
    pub fn refresh(&mut self) {
        self.services = self.discover_services();
        let dir = self.config.bus_type.dir_name();
        let bus_path = format!("/{dir}");
        self.inos.entry(bus_path.clone()).or_insert_with(|| {
            let ino = self.next_service_ino;
            self.next_service_ino += 1;
            self.paths.insert(ino, bus_path.clone());
            ino
        });
        for svc in &self.services {
            let svc_path = format!("{bus_path}/{svc}");
            let svc_ino = match self.inos.get(&svc_path) {
                Some(i) => *i,
                None => {
                    let ino = self.next_service_ino;
                    self.next_service_ino += 1;
                    self.paths.insert(ino, svc_path.clone());
                    self.inos.insert(svc_path.clone(), ino);
                    ino
                }
            };
            for (j, file) in SERVICE_FILES.iter().enumerate() {
                let fpath = format!("{svc_path}/{}", file.name());
                let f_ino = self
                    .inos
                    .entry(fpath.clone())
                    .or_insert(svc_ino * 1000 + j as u64 + 1);
                self.paths.entry(*f_ino).or_insert_with(|| fpath.clone());
            }
        }
    }

    fn discover_services(&self) -> Vec<String> {
        if let Some(b) = &self.bridge {
            let names = b.list_services();
            if !names.is_empty() {
                return names;
            }
        }
        crate::vtree::fallback_services()
    }

    /// inode → mount path.
    pub fn path_of(&self, ino: u64) -> Option<&str> {
        self.paths.get(&ino).map(String::as_str)
    }

    fn log_op(&self, op: &str, path: &str, size: u64) {
        if let Some(chain) = &self.provenance {
            chain
                .lock()
                .expect("provenance lock")
                .log(op, path, "ok", size);
        }
    }

    fn pre(&self, op: &str, path: &str) -> bool {
        if !self.config.enable_contracts {
            return true;
        }
        let c = Contract {
            kind: Kind::Pre,
            operation: op,
            path,
            check: Check::PathResolvable,
        };
        validate(&c) == Verdict::Pass
    }

    fn post(&self, op: &str, path: &str, ok: bool) {
        if !self.config.enable_contracts {
            return;
        }
        let c = Contract {
            kind: Kind::Post,
            operation: op,
            path,
            check: Check::OperationSucceeded { ok },
        };
        let _ = validate(&c);
    }

    /// File content for a virtual file (D-Bus when available, info/demo strings otherwise).
    pub fn content_for(&self, path: &str) -> String {
        match VPath::parse(path) {
            VPath::File(_, service, kind) => {
                let opath = crate::vtree::service_to_object_path(&service);
                match (&self.bridge, kind) {
                    (Some(b), FileKind::Introspect) => match b.introspect(&service, &opath) {
                        Ok(xml) => xml,
                        Err(e) => format!("<!-- Introspect failed: {e} -->\n"),
                    },
                    (Some(b), FileKind::Properties) => {
                        b.properties(&service, &opath).unwrap_or_else(|e| format!("(error: {e})\n"))
                    }
                    (Some(b), FileKind::Interfaces) => {
                        let xml = b.introspect(&service, &opath).unwrap_or_default();
                        let mut lines: Vec<String> = crate::bridge::xml_interfaces(&xml);
                        if lines.is_empty() {
                            "(no interfaces)\n".into()
                        } else {
                            lines.push(String::new());
                            lines.join("\n")
                        }
                    }
                    (Some(b), FileKind::Signals) => {
                        let xml = b.introspect(&service, &opath).unwrap_or_default();
                        let mut lines: Vec<String> = crate::bridge::xml_signals(&xml);
                        if lines.is_empty() {
                            "(no signals)\n".into()
                        } else {
                            lines.push(String::new());
                            lines.join("\n")
                        }
                    }
                    (_, FileKind::Methods) => format!(
                        "stint-busfs: D-Bus service file\n  service:  {service}\n  object:   {opath}\n  file:     methods\n  echo \"MethodName\" > methods to call\n"
                    ),
                    (None, _) => format!(
                        "stint-busfs: D-Bus service file\n  service:  {service}\n  object:   {opath}\n  file:     {}\n  (demo mode: no bus)\n",
                        kind.name()
                    ),
                }
            }
            _ => format!(
                "stint-busfs: D-Bus virtual filesystem\n  path:     {path}\n  dbus:     {}\n  contracts: {}\n  chain:    {} entries\n  mode:     {}\n",
                if self.bridge.is_some() { "connected" } else { "demo (no bus)" },
                if self.config.enable_contracts { "on" } else { "off" },
                self.provenance
                    .as_ref()
                    .map(|c| c.lock().expect("provenance lock").total())
                    .unwrap_or(0),
                if self.config.enable_signals { "async" } else { "sync" },
            ),
        }
    }

    fn attr_for(&self, ino: u64, path: &str) -> Option<FileAttr> {
        let vp = VPath::parse(path);
        let kind = match vp {
            VPath::Root | VPath::Bus(_) | VPath::Service(..) => FileType::Directory,
            VPath::File(..) => FileType::RegularFile,
            VPath::Unknown => return None,
        };
        let size = match kind {
            FileType::Directory => 4096,
            FileType::RegularFile => 4096,
            _ => 0,
        };
        let (perm, nlink) = if kind == FileType::Directory {
            (0o555, 2)
        } else {
            (0o444, 1)
        };
        Some(FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: now(),
            mtime: now(),
            ctime: now(),
            crtime: UNIX_EPOCH,
            kind,
            perm,
            nlink,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 4096,
            flags: 0,
        })
    }
}

fn kernel_config(_cfg: &mut KernelConfig) {}

impl Filesystem for BusFs {
    fn init(&mut self, _req: &Request<'_>, _config: &mut KernelConfig) -> Result<(), libc::c_int> {
        kernel_config(_config);
        Ok(())
    }

    fn destroy(&mut self) {
        if let Some(chain) = &self.provenance {
            let c = chain.lock().expect("provenance lock");
            eprintln!(
                "stint-busfs: provenance chain sealed: {} records, head={}",
                c.total(),
                &c.head_hex()[..16.min(c.head_hex().len())]
            );
        }
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let Some(parent_path) = self.path_of(parent).map(str::to_string) else {
            reply.error(libc::ENOENT);
            return;
        };
        let path = format!(
            "{}/{}",
            parent_path.trim_end_matches('/'),
            name.to_string_lossy()
        );
        match self.inos.get(&path) {
            Some(&ino) => {
                if self.pre("lookup", &path) {
                    if let Some(attr) = self.attr_for(ino, &path) {
                        reply.entry(&TTL, &attr, 0);
                        self.post("lookup", &path, true);
                        self.log_op("lookup", &path, 0);
                        return;
                    }
                }
                self.post("lookup", &path, false);
                reply.error(libc::EACCES);
            }
            None => reply.error(libc::ENOENT),
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _lock: Option<u64>, reply: ReplyAttr) {
        let Some(path) = self.path_of(ino).map(str::to_string) else {
            reply.error(libc::ENOENT);
            return;
        };
        match self.attr_for(ino, &path) {
            Some(attr) => {
                reply.attr(&TTL, &attr);
                self.log_op("getattr", &path, 0);
            }
            None => reply.error(libc::ENOENT),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let Some(path) = self.path_of(ino).map(str::to_string) else {
            reply.error(libc::ENOENT);
            return;
        };
        let vp = VPath::parse(&path);
        let mut entries: Vec<(u64, FileType, String)> = vec![
            (ino, FileType::Directory, ".".into()),
            (
                self.inos.get("/").copied().unwrap_or(ROOT_INO),
                FileType::Directory,
                "..".into(),
            ),
        ];
        for (name, is_dir) in vp.entries(&self.services) {
            let full = if path == "/" {
                format!("/{name}")
            } else {
                format!("{path}/{name}")
            };
            let child_ino = self.inos.get(&full).copied().unwrap_or(0);
            entries.push((
                child_ino,
                if is_dir {
                    FileType::Directory
                } else {
                    FileType::RegularFile
                },
                name,
            ));
        }
        for (i, (e_ino, e_type, e_name)) in
            entries.into_iter().enumerate().skip(offset.max(0) as usize)
        {
            if reply.add(e_ino, (i + 1) as i64, e_type, e_name) {
                break;
            }
        }
        reply.ok();
        self.log_op("readdir", &path, 0);
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        let Some(path) = self.path_of(ino).map(str::to_string) else {
            reply.error(libc::ENOENT);
            return;
        };
        if self.pre("open", &path) {
            self.post("open", &path, true);
            self.log_op("open", &path, 0);
            reply.opened(0, 0);
        } else {
            self.post("open", &path, false);
            reply.error(libc::EACCES);
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(path) = self.path_of(ino).map(str::to_string) else {
            reply.error(libc::ENOENT);
            return;
        };
        if !self.pre("read", &path) {
            reply.error(libc::EACCES);
            return;
        }
        let content = self.content_for(&path);
        let bytes = content.as_bytes();
        let start = (offset.max(0) as usize).min(bytes.len());
        let end = (start + size as usize).min(bytes.len());
        self.post("read", &path, true);
        self.log_op("read", &path, (end - start) as u64);
        reply.data(&bytes[start..end]);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        _offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        let Some(path) = self.path_of(ino).map(str::to_string) else {
            reply.error(libc::ENOENT);
            return;
        };
        if !self.pre("write", &path) {
            reply.error(libc::EACCES);
            return;
        }
        if self.config.enable_contracts {
            let c = Contract {
                kind: Kind::Pre,
                operation: "write",
                path: &path,
                check: Check::SizeWithinBounds {
                    size: data.len(),
                    max: self.config.max_write_size,
                },
            };
            if validate(&c) == Verdict::Violation {
                reply.error(libc::EFBIG);
                return;
            }
        }
        let payload = String::from_utf8_lossy(data);
        let mut parts = payload.split_whitespace();
        if let (Some(method), VPath::File(_, service, FileKind::Methods)) =
            (parts.next(), VPath::parse(&path))
        {
            if let Some(b) = &self.bridge {
                let opath = crate::vtree::service_to_object_path(&service);
                let _ = b.call_method(&service, &opath, method);
            }
        }
        self.post("write", &path, true);
        self.log_op("write", &path, data.len() as u64);
        reply.written(data.len() as u32);
    }

    fn create(
        &mut self,
        _req: &Request<'_>,
        _parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        // the tree is virtual: creation succeeds but the file cannot outlive the path model
        let _ = name;
        reply.created(
            &TTL,
            &FileAttr {
                ino: 0,
                size: 0,
                blocks: 0,
                atime: now(),
                mtime: now(),
                ctime: now(),
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o444,
                nlink: 1,
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                rdev: 0,
                blksize: 4096,
                flags: 0,
            },
            0,
            0,
            0,
        );
    }

    fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
        reply.statfs(
            4096,
            4096,
            1 << 20,
            1 << 20,
            1 << 20,
            1_000_000,
            1_000_000,
            4096,
        );
    }

    fn flush(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BusType, Config};

    fn demo_fs() -> BusFs {
        BusFs::new(
            Config {
                bus_type: BusType::Session,
                ..Config::default()
            },
            None,
        )
    }

    #[test]
    fn stable_inodes_for_the_whole_tree() {
        let fs = demo_fs();
        assert_eq!(fs.path_of(ROOT_INO), Some("/"));
        let svc_ino = *fs.inos.get("/session/org.freedesktop.DBus").unwrap();
        assert!(svc_ino >= 100);
        let f_ino = *fs
            .inos
            .get("/session/org.freedesktop.DBus/introspect")
            .unwrap();
        assert_eq!(
            fs.path_of(f_ino),
            Some("/session/org.freedesktop.DBus/introspect")
        );
        // every service exposes exactly the five virtual files
        for svc in fs.services.clone() {
            for f in SERVICE_FILES {
                let p = format!("/session/{svc}/{}", f.name());
                assert!(fs.inos.contains_key(&p), "missing inode for {p}");
            }
        }
    }

    #[test]
    fn demo_content_falls_back() {
        let fs = demo_fs();
        let info = fs.content_for("/session/org.a/introspect");
        assert!(info.contains("demo mode: no bus"), "{info}");
        let methods = fs.content_for("/session/org.a/methods");
        assert!(
            methods.contains("echo \"MethodName\" > methods"),
            "{methods}"
        );
        let root = fs.content_for("/session");
        assert!(root.contains("D-Bus virtual filesystem") && root.contains("demo (no bus)"));
    }

    #[test]
    fn attrs_distinguish_dirs_and_files() {
        let fs = demo_fs();
        let dir = fs.attr_for(2, "/session").unwrap();
        assert_eq!(dir.kind, FileType::Directory);
        assert_eq!(dir.perm, 0o555);
        let f_ino = *fs
            .inos
            .get("/session/org.freedesktop.DBus/properties")
            .unwrap();
        let file = fs
            .attr_for(f_ino, "/session/org.freedesktop.DBus/properties")
            .unwrap();
        assert_eq!(file.kind, FileType::RegularFile);
        assert_eq!(file.perm, 0o444);
        assert!(fs.attr_for(9999, "/nope").is_none());
    }
}
