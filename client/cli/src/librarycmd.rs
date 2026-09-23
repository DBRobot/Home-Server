//! `dd library`: a member's encrypted libraries from the terminal. The key
//! comes out of the entry (sealed to this device or the root), every byte
//! is encrypted here before it goes anywhere, and the box's gate only ever
//! hands out a url for one object at a time. `dd media` mounts the same
//! libraries; this is the plain path in and out.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use auth::KeyStore as _;
use clap::Subcommand;
use library::{Library, Record};

use crate::who;

#[derive(Subcommand)]
pub enum LibraryCmd {
    /// Make a new library: a key sealed to every key of yours, in your entry
    New,
    /// The libraries you can open
    List,
    /// What a library holds
    Ls { library: String },
    /// A file into a library, under a name (default: the file's name); `-` reads stdin
    Put {
        library: String,
        file: PathBuf,
        #[arg(long = "as")]
        name: Option<String>,
    },
    /// A file out of a library, by name
    Get {
        library: String,
        name: String,
        out: PathBuf,
    },
    /// Move a file to the trash (nothing is gone until the box purges it)
    Trash { library: String, name: String },
}

/// what this machine can open a library with
pub struct Opener {
    device: (String, ed25519_dalek::SigningKey),
    root: Option<ed25519_dalek::SigningKey>,
}

impl Opener {
    pub fn load(keys: &auth::Store) -> Result<(Self, String, String)> {
        let kp = auth::device::load(keys)?.context("no device key here - `dd device show`")?;
        let user = keys
            .get(crate::USER)?
            .context("no name on this machine - `dd identity new` or `dd identity import`")?
            .to_string();
        let public = auth::device::public_b64(&kp);
        let seed: [u8; 32] = kp.private().to_bytes()[..]
            .try_into()
            .context("device key is not 32 bytes")?;
        let token = auth::device::mint(&kp, &user, Duration::from_secs(3600))?;
        Ok((
            Opener {
                device: (
                    identity::fingerprint(&public),
                    ed25519_dalek::SigningKey::from_bytes(&seed),
                ),
                root: who::load_root(keys)?,
            },
            user,
            token,
        ))
    }
    fn open(&self, lib: &Library) -> Result<library::Key> {
        library::open_library(
            lib,
            Some((&self.device.0, &self.device.1)),
            self.root.as_ref(),
        )
        .with_context(|| format!("library {}: no key of this machine opens it", lib.id))
    }
}

/// the gate on the box, for one library
pub struct Gate {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl Gate {
    pub fn new(files_base: &str, lib: &str, token: &str) -> Self {
        Gate {
            base: format!("{}/_dd/library/{lib}", files_base.trim_end_matches('/')),
            token: token.to_string(),
            http: reqwest::Client::new(),
        }
    }
    async fn json(&self, method: reqwest::Method, path: &str) -> Result<serde_json::Value> {
        let r = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = r.status();
        let body = r.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("gate: {status} {body}");
        }
        if body.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_str(&body)?)
    }
    async fn url(&self, method: reqwest::Method, object: &str) -> Result<String> {
        let v = self.json(method, &format!("/url/{object}")).await?;
        v["url"]
            .as_str()
            .map(str::to_string)
            .context("gate gave no url")
    }
    async fn record_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let path = match &after {
                Some(a) => format!("/records?after={a}"),
                None => "/records".to_string(),
            };
            let v = self.json(reqwest::Method::GET, &path).await?;
            for id in v["ids"].as_array().into_iter().flatten() {
                if let Some(s) = id.as_str() {
                    ids.push(s.to_string());
                }
            }
            if v["more"].as_bool() != Some(true) {
                break;
            }
            after = v["last"].as_str().map(str::to_string);
        }
        Ok(ids)
    }
    pub async fn fetch(&self, object: &str) -> Result<Vec<u8>> {
        let url = self.url(reqwest::Method::GET, object).await?;
        let r = self.http.get(url).send().await?;
        if !r.status().is_success() {
            bail!("fetching {object}: {}", r.status());
        }
        Ok(r.bytes().await?.to_vec())
    }
    pub async fn store(&self, object: &str, bytes: Vec<u8>) -> Result<()> {
        let url = self.url(reqwest::Method::PUT, object).await?;
        let r = self.http.put(url).body(bytes).send().await?;
        if !r.status().is_success() {
            bail!("storing {object}: {}", r.status());
        }
        Ok(())
    }
    pub async fn records(&self, key: &library::Key) -> Result<Vec<Record>> {
        let mut out = Vec::new();
        for id in self.record_ids().await? {
            let bytes = self.fetch(&library::record_object(&id)).await?;
            out.push(library::open_record(key, &id, &bytes)?);
        }
        Ok(out)
    }
}

/// every library this machine can open: the member's own, then those
/// shared with them by anyone whose entry names them
pub async fn openable(
    dirs: &[String],
    user: &str,
    opener: &Opener,
) -> Result<Vec<(String, Library, library::Key)>> {
    let mut out = Vec::new();
    let found = who::fetch(dirs, user).await;
    let mine = who::newest(&found).context("no directory has your entry")?;
    for lib in &mine.entry.libraries {
        if let Ok(k) = opener.open(lib) {
            out.push((user.to_string(), lib.clone(), k));
        }
    }
    // shared with us: a reader's keys are sealed the same way, under the
    // owner's entry. The gate lists every entry; a reader looks through
    // them once
    if let Some(dir) = dirs.first()
        && let Ok(names) = who::names(dir).await
    {
        for name in names.into_iter().filter(|n| n != user) {
            let f = who::fetch(dirs, &name).await;
            let Some(e) = who::newest(&f) else { continue };
            for lib in &e.entry.libraries {
                if let Some(r) = lib.readers.iter().find(|r| r.name == user) {
                    let as_mine = Library {
                        keys: r.keys.clone(),
                        ..lib.clone()
                    };
                    if let Ok(k) = opener.open(&as_mine) {
                        out.push((name.clone(), lib.clone(), k));
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn files_base(dirs: &[String]) -> Result<String> {
    // the gate is on the same host as the directory: https://files.<base>
    let d = dirs.first().context("no directory")?;
    Ok(d.trim_end_matches("/_dd/directory").to_string())
}

pub async fn run(cmd: LibraryCmd, keys: &auth::Store, dirs: &[String]) -> Result<()> {
    let (opener, user, token) = Opener::load(keys)?;
    let base = files_base(dirs)?;
    match cmd {
        LibraryCmd::New => {
            let root = opener
                .root
                .as_ref()
                .context("the root key is not on this machine: a library is made where it is")?;
            let cur = who::ours(dirs, &user, root).await?;
            let key = library::random_key();
            let lib = Library {
                id: library::random_id(),
                keys: library::seal_for_entry(&cur.entry, &key)?,
                readers: vec![],
                created: identity::now(),
            };
            let id = lib.id.clone();
            who::add_library(dirs, &user, root, lib).await?;
            println!(
                "library {id}: sealed to {} of your keys, in your entry",
                cur.entry.devices.len() + 2
            );
        }
        LibraryCmd::List => {
            let libs = openable(dirs, &user, &opener).await?;
            if libs.is_empty() {
                println!("no libraries - `dd library new` makes one");
            }
            for (owner, lib, _) in libs {
                let whose = if owner == user {
                    "yours".to_string()
                } else {
                    format!("{owner}'s")
                };
                println!("{}  {whose}  {} key(s)", lib.id, lib.keys.len());
            }
        }
        LibraryCmd::Ls { library } => {
            let (_, key) = pick(dirs, &user, &opener, &library).await?;
            let gate = Gate::new(&base, &library, &token);
            let mut recs = gate.records(&key).await?;
            recs.sort_by(|a, b| a.name.cmp(&b.name));
            for r in recs {
                println!("{:>12}  {}", r.size, r.name);
            }
        }
        LibraryCmd::Put {
            library,
            file,
            name,
        } => {
            let (_, key) = pick(dirs, &user, &opener, &library).await?;
            let gate = Gate::new(&base, &library, &token);
            let name = name.unwrap_or_else(|| {
                file.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            if name.is_empty() {
                bail!("a file from stdin needs a name: --as <name>");
            }
            // streamed a chunk at a time: a film does not fit in memory, and
            // `-` lets a copy come in over a pipe (ssh box cat ...)
            let mut src: Box<dyn std::io::Read> = if file.as_os_str() == "-" {
                Box::new(std::io::stdin())
            } else {
                Box::new(
                    std::fs::File::open(&file)
                        .with_context(|| format!("opening {}", file.display()))?,
                )
            };
            let file_key = library::random_key();
            let id = library::random_id();
            let mut n = 0u64;
            let mut size = 0u64;
            let mut buf = vec![0u8; library::CHUNK];
            loop {
                let got = read_full(&mut src, &mut buf)?;
                if got == 0 && n > 0 {
                    break;
                }
                gate.store(
                    &library::chunk_object(&id, n),
                    library::seal_chunk(&file_key, n, &buf[..got])?,
                )
                .await?;
                n += 1;
                size += got as u64;
                eprint!("\r{name}: chunk {n} ({} MiB)", size >> 20);
                if got < library::CHUNK {
                    break;
                }
            }
            eprintln!();
            let rec = Record {
                id: id.clone(),
                name: name.clone(),
                size,
                chunks: n,
                key: library::encode_key(&file_key),
                modified: identity::now(),
            };
            gate.store(
                &library::record_object(&id),
                library::seal_record(&key, &rec)?,
            )
            .await?;
            println!("{name}: {size} bytes in {n} chunk(s), record {id}");
        }
        LibraryCmd::Get { library, name, out } => {
            let (_, key) = pick(dirs, &user, &opener, &library).await?;
            let gate = Gate::new(&base, &library, &token);
            let rec = gate
                .records(&key)
                .await?
                .into_iter()
                .find(|r| r.name == name)
                .with_context(|| format!("no {name} in {library}"))?;
            let file_key = library::file_key(&rec)?;
            let mut f = std::fs::File::create(&out)?;
            use std::io::Write as _;
            for n in 0..rec.chunks {
                let sealed = gate.fetch(&library::chunk_object(&rec.id, n)).await?;
                f.write_all(&library::open_chunk(&file_key, n, &sealed)?)?;
                eprint!("\r{name}: chunk {}/{}", n + 1, rec.chunks);
            }
            eprintln!();
            println!("{name}: {} bytes -> {}", rec.size, out.display());
        }
        LibraryCmd::Trash { library, name } => {
            let (_, key) = pick(dirs, &user, &opener, &library).await?;
            let gate = Gate::new(&base, &library, &token);
            let rec = gate
                .records(&key)
                .await?
                .into_iter()
                .find(|r| r.name == name)
                .with_context(|| format!("no {name} in {library}"))?;
            gate.json(reqwest::Method::POST, &format!("/trash/{}", rec.id))
                .await?;
            println!("{name}: in the trash; the chunks stay until the box purges");
        }
    }
    Ok(())
}

/// as much of `buf` as the reader gives before it ends
fn read_full(r: &mut dyn std::io::Read, buf: &mut [u8]) -> Result<usize> {
    let mut at = 0;
    while at < buf.len() {
        match r.read(&mut buf[at..]) {
            Ok(0) => break,
            Ok(k) => at += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(at)
}

async fn pick(
    dirs: &[String],
    user: &str,
    opener: &Opener,
    library: &str,
) -> Result<(Library, library::Key)> {
    openable(dirs, user, opener)
        .await?
        .into_iter()
        .find(|(_, l, _)| l.id == library)
        .map(|(_, l, k)| (l, k))
        .with_context(|| format!("no library {library} that this machine can open"))
}
