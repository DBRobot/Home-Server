//! The libraries as folders: every library this machine can open, on this
//! machine, and a player pointed at them. The box holds ciphertext; the
//! folders are where it becomes films, series, files. Reads fetch a chunk
//! at a time through the gate and keep a cache on this disk; a file
//! written into a folder is encrypted and uploaded when it is closed.
//! Nothing here can delete: `dd library trash` is the deliberate act.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fuser::{
    FileAttr, FileType, Filesystem, INodeNo, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
};
use library::Record;

use crate::gate::{self, Gate, Opener};

const TTL: Duration = Duration::from_secs(1);
use fuser::Errno;
const EPERM: Errno = Errno::EPERM;
const ENOENT: Errno = Errno::ENOENT;
const EIO: Errno = Errno::EIO;

/// what an inode is
#[derive(Clone)]
enum Node {
    Root,
    /// a library's top folder
    Library(usize),
    /// a folder inside a library, by path
    Dir(usize, String),
    /// a file: which library, which record
    File(usize, String),
    /// a file being written, not yet on the box (its name is in `drafts`)
    Draft(usize),
}

struct Lib {
    id: String,
    key: library::Key,
    gate: Gate,
    records: Vec<Record>,
}

struct Draft {
    path: PathBuf,
    name: String,
}

/// everything the callbacks touch, behind one lock: fuser calls with a
/// shared reference, and a mount is not busy enough for finer locking
struct State {
    rt: tokio::runtime::Handle,
    libs: Vec<Lib>,
    cache: PathBuf,
    nodes: HashMap<u64, Node>,
    by_key: HashMap<String, u64>,
    next: u64,
    drafts: HashMap<u64, Draft>,
    uid: u32,
    gid: u32,
}

fn now() -> SystemTime {
    SystemTime::now()
}

struct MediaFs(std::sync::Mutex<State>);

impl State {
    fn inode(&mut self, key: String, node: Node) -> u64 {
        if let Some(i) = self.by_key.get(&key) {
            return *i;
        }
        let i = self.next;
        self.next += 1;
        self.nodes.insert(i, node);
        self.by_key.insert(key, i);
        i
    }

    fn attr(&self, ino: u64, node: &Node) -> FileAttr {
        let (kind, size, mtime) = match node {
            Node::Root | Node::Library(_) | Node::Dir(..) => (FileType::Directory, 0, UNIX_EPOCH),
            Node::File(l, id) => {
                let r = self.libs[*l].records.iter().find(|r| &r.id == id);
                (
                    FileType::RegularFile,
                    r.map(|r| r.size).unwrap_or(0),
                    UNIX_EPOCH + Duration::from_secs(r.map(|r| r.modified).unwrap_or(0)),
                )
            }
            Node::Draft(..) => (
                FileType::RegularFile,
                self.drafts
                    .get(&ino)
                    .and_then(|d| std::fs::metadata(&d.path).ok())
                    .map(|m| m.len())
                    .unwrap_or(0),
                now(),
            ),
        };
        FileAttr {
            ino: INodeNo(ino),
            size,
            blocks: size.div_ceil(512),
            atime: mtime,
            mtime,
            ctime: mtime,
            crtime: mtime,
            kind,
            perm: if kind == FileType::Directory {
                0o755
            } else {
                0o644
            },
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// the children of a folder in a library: files whose name is directly
    /// under it, and the next path element of deeper ones
    fn children(&mut self, l: usize, dir: &str) -> Vec<(u64, FileType, String)> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out: Vec<(u64, FileType, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let entries: Vec<(String, String)> = self.libs[l]
            .records
            .iter()
            .filter(|r| r.name.starts_with(&prefix))
            .map(|r| (r.name[prefix.len()..].to_string(), r.id.clone()))
            .collect();
        for (rest, id) in entries {
            match rest.split_once('/') {
                None => {
                    if seen.insert(rest.clone()) {
                        let ino = self.inode(format!("f:{l}:{id}"), Node::File(l, id));
                        out.push((ino, FileType::RegularFile, rest));
                    }
                }
                Some((first, _)) => {
                    if seen.insert(first.to_string()) {
                        let path = format!("{prefix}{first}");
                        let ino = self.inode(format!("d:{l}:{path}"), Node::Dir(l, path));
                        out.push((ino, FileType::Directory, first.to_string()));
                    }
                }
            }
        }
        // drafts being written here
        let drafts: Vec<(u64, String)> = self
            .drafts
            .iter()
            .filter(|(_, d)| d.name.starts_with(&prefix) && !d.name[prefix.len()..].contains('/'))
            .map(|(i, d)| (*i, d.name[prefix.len()..].to_string()))
            .collect();
        for (ino, name) in drafts {
            if seen.insert(name.clone()) {
                out.push((ino, FileType::RegularFile, name));
            }
        }
        out.sort_by(|a, b| a.2.cmp(&b.2));
        out
    }

    fn library_dir_name(&self, l: usize) -> String {
        self.libs[l].id[..12].to_string()
    }

    /// a decrypted chunk, from the cache or the box
    fn chunk(&self, l: usize, rec: &Record, n: u64) -> std::result::Result<Vec<u8>, Errno> {
        let path = self
            .cache
            .join(&self.libs[l].id)
            .join(&rec.id)
            .join(format!("{n:08}"));
        if let Ok(b) = std::fs::read(&path) {
            return Ok(b);
        }
        let gate = &self.libs[l].gate;
        let sealed = self
            .rt
            .block_on(gate.fetch(&library::chunk_object(&rec.id, n)))
            .map_err(|e| {
                eprintln!("dd media: fetching {} chunk {n}: {e:#}", rec.name);
                EIO
            })?;
        let key = library::file_key(rec).map_err(|_| EIO)?;
        let plain = library::open_chunk(&key, n, &sealed).map_err(|_| EIO)?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, &plain);
        Ok(plain)
    }

    /// a closed draft goes to the box: chunks, then the record
    fn upload(&mut self, ino: u64) -> std::result::Result<(), Errno> {
        let Some(d) = self.drafts.remove(&ino) else {
            return Ok(());
        };
        let Some(Node::Draft(l)) = self.nodes.get(&ino).cloned() else {
            return Ok(());
        };
        let data = std::fs::read(&d.path).map_err(|_| EIO)?;
        let _ = std::fs::remove_file(&d.path);
        let file_key = library::random_key();
        let id = library::random_id();
        let gate = &self.libs[l].gate;
        let mut n = 0u64;
        for chunk in data.chunks(library::CHUNK) {
            let sealed = library::seal_chunk(&file_key, n, chunk).map_err(|_| EIO)?;
            self.rt
                .block_on(gate.store(&library::chunk_object(&id, n), sealed))
                .map_err(|e| {
                    eprintln!("dd media: uploading {}: {e:#}", d.name);
                    EIO
                })?;
            n += 1;
        }
        let rec = Record {
            id: id.clone(),
            name: d.name.clone(),
            size: data.len() as u64,
            chunks: n,
            key: library::encode_key(&file_key),
            modified: identity::now(),
        };
        let sealed = library::seal_record(&self.libs[l].key, &rec).map_err(|_| EIO)?;
        self.rt
            .block_on(gate.store(&library::record_object(&id), sealed))
            .map_err(|_| EIO)?;
        eprintln!(
            "dd media: {} stored, {} bytes in {n} chunk(s)",
            d.name,
            data.len()
        );
        // the draft becomes the file it now is
        self.libs[l].records.push(rec);
        self.by_key.retain(|_, v| *v != ino);
        self.nodes.insert(ino, Node::File(l, id.clone()));
        self.by_key.insert(format!("f:{l}:{id}"), ino);
        Ok(())
    }
}

impl Filesystem for MediaFs {
    fn lookup(&self, _req: &fuser::Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let mut st = self.0.lock().unwrap();
        let name = name.to_string_lossy().into_owned();
        let Some(pn) = st.nodes.get(&parent.0).cloned() else {
            return reply.error(ENOENT);
        };
        let found = match pn {
            Node::Root => (0..st.libs.len())
                .find(|l| st.library_dir_name(*l) == name)
                .map(|l| st.inode(format!("l:{l}"), Node::Library(l))),
            Node::Library(l) => st
                .children(l, "")
                .into_iter()
                .find(|c| c.2 == name)
                .map(|c| c.0),
            Node::Dir(l, path) => st
                .children(l, &path)
                .into_iter()
                .find(|c| c.2 == name)
                .map(|c| c.0),
            _ => None,
        };
        match found {
            Some(ino) => {
                let node = st.nodes[&ino].clone();
                reply.entry(&TTL, &st.attr(ino, &node), fuser::Generation(0))
            }
            None => reply.error(ENOENT),
        }
    }

    fn getattr(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        _fh: Option<fuser::FileHandle>,
        reply: ReplyAttr,
    ) {
        let st = self.0.lock().unwrap();
        match st.nodes.get(&ino.0).cloned() {
            Some(n) => reply.attr(&TTL, &st.attr(ino.0, &n)),
            None => reply.error(ENOENT),
        }
    }

    fn readdir(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let mut st = self.0.lock().unwrap();
        let Some(node) = st.nodes.get(&ino.0).cloned() else {
            return reply.error(ENOENT);
        };
        let mut entries: Vec<(u64, FileType, String)> =
            vec![(ino.0, FileType::Directory, ".".into())];
        entries.push((1, FileType::Directory, "..".into()));
        match node {
            Node::Root => {
                for l in 0..st.libs.len() {
                    let name = st.library_dir_name(l);
                    let i = st.inode(format!("l:{l}"), Node::Library(l));
                    entries.push((i, FileType::Directory, name));
                }
            }
            Node::Library(l) => entries.extend(st.children(l, "")),
            Node::Dir(l, path) => entries.extend(st.children(l, &path)),
            _ => return reply.error(ENOENT),
        }
        for (i, (n, kind, name)) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(INodeNo(n), (i + 1) as u64, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn read(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        size: u32,
        _flags: fuser::OpenFlags,
        _lock: Option<fuser::LockOwner>,
        reply: ReplyData,
    ) {
        let st = self.0.lock().unwrap();
        let Some(node) = st.nodes.get(&ino.0).cloned() else {
            return reply.error(ENOENT);
        };
        match node {
            Node::File(l, id) => {
                let Some(rec) = st.libs[l].records.iter().find(|r| r.id == id).cloned() else {
                    return reply.error(ENOENT);
                };
                let start = offset;
                if start >= rec.size {
                    return reply.data(&[]);
                }
                let end = (start + size as u64).min(rec.size);
                let mut out = Vec::with_capacity((end - start) as usize);
                let mut pos = start;
                while pos < end {
                    let n = pos / library::CHUNK as u64;
                    let within = (pos % library::CHUNK as u64) as usize;
                    let chunk = match st.chunk(l, &rec, n) {
                        Ok(c) => c,
                        Err(e) => return reply.error(e),
                    };
                    let take = ((end - pos) as usize).min(chunk.len().saturating_sub(within));
                    if take == 0 {
                        break;
                    }
                    out.extend_from_slice(&chunk[within..within + take]);
                    pos += take as u64;
                }
                reply.data(&out);
            }
            Node::Draft(..) => match st.drafts.get(&ino.0) {
                Some(d) => {
                    use std::io::{Read as _, Seek as _};
                    let mut f = match std::fs::File::open(&d.path) {
                        Ok(f) => f,
                        Err(_) => return reply.error(EIO),
                    };
                    let mut buf = vec![0u8; size as usize];
                    let _ = f.seek(std::io::SeekFrom::Start(offset));
                    let n = f.read(&mut buf).unwrap_or(0);
                    reply.data(&buf[..n]);
                }
                None => reply.error(ENOENT),
            },
            _ => reply.error(Errno::EISDIR),
        }
    }

    fn create(
        &self,
        _req: &fuser::Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: fuser::ReplyCreate,
    ) {
        let mut st = self.0.lock().unwrap();
        let name = name.to_string_lossy().into_owned();
        let (l, dir) = match st.nodes.get(&parent.0).cloned() {
            Some(Node::Library(l)) => (l, String::new()),
            Some(Node::Dir(l, p)) => (l, p),
            _ => return reply.error(EPERM),
        };
        let full = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        let path = st.cache.join("drafts").join(library::random_id());
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::File::create(&path).is_err() {
            return reply.error(EIO);
        }
        let key = format!("w:{l}:{full}:{}", st.next);
        let ino = st.inode(key, Node::Draft(l));
        st.drafts.insert(ino, Draft { path, name: full });
        let node = st.nodes[&ino].clone();
        reply.created(
            &TTL,
            &st.attr(ino, &node),
            fuser::Generation(0),
            fuser::FileHandle(0),
            fuser::FopenFlags::empty(),
        );
    }

    fn write(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: fuser::WriteFlags,
        _flags: fuser::OpenFlags,
        _lock: Option<fuser::LockOwner>,
        reply: fuser::ReplyWrite,
    ) {
        let st = self.0.lock().unwrap();
        let Some(d) = st.drafts.get(&ino.0) else {
            return reply.error(EPERM);
        };
        use std::io::{Seek as _, Write as _};
        let mut f = match std::fs::OpenOptions::new().write(true).open(&d.path) {
            Ok(f) => f,
            Err(_) => return reply.error(EIO),
        };
        if f.seek(std::io::SeekFrom::Start(offset)).is_err() || f.write_all(data).is_err() {
            return reply.error(EIO);
        }
        reply.written(data.len() as u32);
    }

    fn setattr(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<fuser::FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let st = self.0.lock().unwrap();
        if let (Some(sz), Some(d)) = (size, st.drafts.get(&ino.0))
            && let Ok(f) = std::fs::OpenOptions::new().write(true).open(&d.path)
        {
            let _ = f.set_len(sz);
        }
        match st.nodes.get(&ino.0).cloned() {
            Some(n) => reply.attr(&TTL, &st.attr(ino.0, &n)),
            None => reply.error(ENOENT),
        }
    }

    fn release(
        &self,
        _req: &fuser::Request,
        ino: INodeNo,
        _fh: fuser::FileHandle,
        _flags: fuser::OpenFlags,
        _lock: Option<fuser::LockOwner>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        let mut st = self.0.lock().unwrap();
        if st.drafts.contains_key(&ino.0) {
            return match st.upload(ino.0) {
                Ok(()) => reply.ok(),
                Err(e) => reply.error(e),
            };
        }
        reply.ok()
    }

    fn mkdir(
        &self,
        _req: &fuser::Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        // folders are what file names say; an empty one lives only until
        // something is written into it
        let mut st = self.0.lock().unwrap();
        let name = name.to_string_lossy().into_owned();
        let (l, dir) = match st.nodes.get(&parent.0).cloned() {
            Some(Node::Library(l)) => (l, String::new()),
            Some(Node::Dir(l, p)) => (l, p),
            _ => return reply.error(EPERM),
        };
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        let ino = st.inode(format!("d:{l}:{path}"), Node::Dir(l, path));
        let node = st.nodes[&ino].clone();
        reply.entry(&TTL, &st.attr(ino, &node), fuser::Generation(0))
    }

    // nothing here removes anything: that is `dd library trash`, on purpose
    fn unlink(
        &self,
        _req: &fuser::Request,
        _parent: INodeNo,
        _name: &OsStr,
        reply: fuser::ReplyEmpty,
    ) {
        reply.error(EPERM)
    }
    fn rmdir(
        &self,
        _req: &fuser::Request,
        _parent: INodeNo,
        _name: &OsStr,
        reply: fuser::ReplyEmpty,
    ) {
        reply.error(EPERM)
    }
    fn rename(
        &self,
        _req: &fuser::Request,
        _parent: INodeNo,
        _name: &OsStr,
        _newparent: INodeNo,
        _newname: &OsStr,
        _flags: fuser::RenameFlags,
        reply: fuser::ReplyEmpty,
    ) {
        reply.error(EPERM)
    }
}

/// what a mount holds, for the caller to say
pub struct Mounted {
    pub id: String,
    pub owner: String,
    pub files: usize,
}

/// a live mount; dropping it unmounts
pub struct Mount {
    pub at: PathBuf,
    pub libraries: Vec<Mounted>,
    _session: fuser::BackgroundSession,
}

/// mount every openable library under `at`. Needs a tokio runtime (the
/// callbacks block on it for chunks) and fuse (fusermount3) on the machine.
pub async fn mount(keys: &impl auth::KeyStore, dirs: &[String], at: PathBuf) -> Result<Mount> {
    let (opener, user, token) = Opener::load(keys)?;
    let base = gate::files_base(dirs)?;
    let home = std::env::var("HOME").context("HOME")?;
    let cache = PathBuf::from(&home).join(".cache/dd/library");
    // a mount left by a process that died is a folder nothing can open;
    // unmounting it first is what the person would do by hand
    if std::fs::read_dir(&at).is_err() {
        let _ = std::process::Command::new("fusermount3")
            .arg("-uz")
            .arg(&at)
            .stderr(std::process::Stdio::null())
            .status();
    }
    std::fs::create_dir_all(&at)?;
    std::fs::create_dir_all(&cache)?;
    let mut libs = Vec::new();
    let mut mounted = Vec::new();
    for (owner, lib, key) in gate::openable(dirs, &user, &opener).await? {
        let gate = Gate::new(&base, &lib.id, &token);
        let records = gate
            .records(&key)
            .await
            .with_context(|| format!("library {}", lib.id))?;
        mounted.push(Mounted {
            id: lib.id.clone(),
            owner: owner.clone(),
            files: records.len(),
        });
        libs.push(Lib {
            id: lib.id,
            key,
            gate,
            records,
        });
    }
    if libs.is_empty() {
        anyhow::bail!("no library to mount - `dd library new` makes one");
    }
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let mut st = State {
        rt: tokio::runtime::Handle::current(),
        libs,
        cache,
        nodes: HashMap::new(),
        by_key: HashMap::new(),
        next: 2,
        drafts: HashMap::new(),
        uid,
        gid,
    };
    st.nodes.insert(1, Node::Root);
    let fs = MediaFs(std::sync::Mutex::new(st));
    let mut config = fuser::Config::default();
    config.mount_options = vec![
        fuser::MountOption::FSName("commonty".into()),
        fuser::MountOption::RW,
        fuser::MountOption::DefaultPermissions,
        fuser::MountOption::NoAtime,
    ];
    let session = fuser::spawn_mount(fs, &at, &config).with_context(|| {
        format!(
            "mounting at {}: is fuse available (fusermount3), and is the folder empty?",
            at.display()
        )
    })?;
    Ok(Mount {
        at,
        libraries: mounted,
        _session: session,
    })
}
