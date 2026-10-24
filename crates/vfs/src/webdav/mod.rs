//! WebDAV (RFC 4918) over HTTP or HTTPS: Nextcloud, ownCloud, Synology /
//! QNAP, Apache `mod_dav`, nginx, rclone, ...
//!
//! - Listing: `PROPFIND` with `Depth: 1` (resource type, size, mtime, ETag).
//! - Reading: `GET`, with `Range` to continue an interrupted download.
//! - Writing: `PUT`. Files up to [`BUFFERED_MAX`] go in one request with a
//!   `Content-Length`; larger ones are streamed (chunked) while the copy
//!   runs, so the progress is real. Uploads restart from the beginning
//!   after an interruption (WebDAV has no standard way to append).
//! - `MKCOL`, `DELETE`, `MOVE` (rename), `COPY` (server-side copy),
//!   `PROPPATCH` for the modification time where the server allows it.
//! - Basic and Digest (MD5 / SHA-256, `qop=auth`) authentication.
//!
//! The connection's `webdav.root` (e.g. `/remote.php/dav/files/alice`) is
//! Manta's `/`.

mod client;
pub mod xml;

#[cfg(test)]
mod fake_server;
#[cfg(test)]
mod tests;

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

use chrono::{DateTime, Utc};
use manta_core::{
    Auth, ConnectionConfig, Entry, EntryKind, Protocol, ResumeCaps, Vfs, VfsError, VfsPath,
    VfsResult,
};

pub use client::{decode, encode_path, encode_segment, normalize_root, Client};
use client::{status_error, Body};
use xml::DavItem;

/// Uploads up to this size are buffered and sent with a `Content-Length`
/// (works with every server); larger ones are streamed.
pub const BUFFERED_MAX: usize = 8 * 1024 * 1024;

/// Root folder presets of the connect dialog: (id, path template).
/// `{user}` is replaced by the (percent-encoded) user name.
pub const PRESETS: &[(&str, &str)] = &[
    ("generic", ""),
    ("nextcloud", "/remote.php/dav/files/{user}"),
    ("owncloud", "/remote.php/dav/files/{user}"),
];

/// The root folder of preset `id` for `user` (`None`: unknown preset or
/// a free-form root).
pub fn preset_root(id: &str, user: &str) -> Option<String> {
    PRESETS
        .iter()
        .find(|(p, _)| *p == id)
        .map(|(_, t)| t.replace("{user}", user.trim()))
}

pub struct WebDavFs {
    client: Arc<Client>,
    label: String,
    protocol: Protocol,
    preset: String,
    /// The server refused to set modification times: don't ask again.
    no_mtime: AtomicBool,
}

fn io_err(e: VfsError) -> io::Error {
    match e {
        VfsError::Io(e) => e,
        VfsError::NotFound(m) => io::Error::new(io::ErrorKind::NotFound, m),
        VfsError::PermissionDenied(m) => io::Error::new(io::ErrorKind::PermissionDenied, m),
        other => io::Error::other(other),
    }
}

impl WebDavFs {
    /// Connect: check the login and that the root folder exists.
    pub fn connect(cfg: &ConnectionConfig) -> VfsResult<WebDavFs> {
        let fs = WebDavFs::new(cfg)?;
        let root = fs.stat(&VfsPath::root()).map_err(|e| match e {
            VfsError::NotFound(_) => VfsError::NotFound(format!(
                "{}: no WebDAV folder at this path (HTTP 404)",
                fs.root_url()
            )),
            e => e,
        })?;
        if !root.is_dir() {
            return Err(VfsError::NotADirectory(format!(
                "{}: the WebDAV folder is a file",
                fs.client.url("/", true)
            )));
        }
        Ok(fs)
    }

    /// The backend for `cfg`, without network access.
    pub fn new(cfg: &ConnectionConfig) -> VfsResult<WebDavFs> {
        let https = match cfg.protocol {
            Protocol::WebDavs => true,
            Protocol::WebDav => false,
            p => return Err(VfsError::Other(format!("not a WebDAV connection: {p}"))),
        };
        if cfg.host.trim().is_empty() {
            return Err(VfsError::Other("no server given".into()));
        }
        let password = match &cfg.auth {
            Auth::Password(p) => Some(p.clone()),
            _ => None,
        };
        let opts = cfg.webdav.as_deref().cloned().unwrap_or_default();
        let client = Client::new(
            https,
            cfg.host.trim(),
            crate::effective_port(cfg),
            &opts.root,
            cfg.username.trim(),
            password,
        );
        let label = if cfg.username.trim().is_empty() {
            format!("{} ({})", cfg.host.trim(), cfg.protocol)
        } else {
            format!(
                "{}@{} ({})",
                cfg.username.trim(),
                cfg.host.trim(),
                cfg.protocol
            )
        };
        Ok(WebDavFs {
            client: Arc::new(client),
            label,
            protocol: cfg.protocol,
            preset: opts.preset,
            no_mtime: AtomicBool::new(false),
        })
    }

    /// The URL of the root folder (for messages and tests).
    pub fn root_url(&self) -> String {
        self.client.url("/", true)
    }

    fn propfind(&self, url: &str, depth: &str, ctx: &str) -> VfsResult<Vec<DavItem>> {
        let headers = [
            ("depth", depth.to_string()),
            ("content-type", "application/xml; charset=utf-8".to_string()),
        ];
        let (status, text) = self.client.send_text(
            "PROPFIND",
            url,
            &headers,
            Body::Bytes(xml::PROPFIND_BODY.as_bytes()),
            ctx,
        )?;
        if status != 207 {
            return Err(VfsError::Other(format!(
                "{ctx}: not a WebDAV folder (the server answered HTTP {status} instead of 207 Multi-Status)"
            )));
        }
        xml::parse_multistatus(&text).map_err(|e| VfsError::Other(format!("{ctx}: {e}")))
    }

    fn entry(path: &VfsPath, item: &DavItem) -> Entry {
        let name = path.file_name().to_string();
        Entry {
            hidden: name.starts_with('.'),
            name,
            path: path.clone(),
            kind: if item.is_dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            size: if item.is_dir { 0 } else { item.size },
            modified: item.modified,
            permissions: None,
            link_target: None,
        }
    }

    /// PROPFIND `Depth: 0` of one path.
    fn stat_item(&self, path: &VfsPath) -> VfsResult<DavItem> {
        let ctx = path.to_string();
        let url = self.client.url(path.as_str(), path.is_root());
        self.propfind(&url, "0", &ctx)?
            .into_iter()
            .next()
            .ok_or(VfsError::NotFound(ctx))
    }

    fn put_bytes(&self, path: &VfsPath, data: &[u8]) -> VfsResult<()> {
        let url = self.client.url(path.as_str(), false);
        self.client.send(
            "PUT",
            &url,
            &[("content-type", "application/octet-stream".into())],
            Body::Bytes(data),
            path.as_str(),
        )?;
        Ok(())
    }

    fn proppatch(&self, path: &VfsPath, name: &str, value: &str) -> VfsResult<bool> {
        let url = self.client.url(path.as_str(), false);
        let body = xml::proppatch_body(name, value);
        let resp = self.client.send_raw(
            "PROPPATCH",
            &url,
            &[("content-type", "application/xml; charset=utf-8".into())],
            Body::Bytes(body.as_bytes()),
            path.as_str(),
        )?;
        let status = resp.status().as_u16();
        if status == 404 {
            return Err(VfsError::NotFound(path.to_string()));
        }
        if status != 207 {
            return Ok((200..300).contains(&status));
        }
        let mut resp = resp;
        let text = resp
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .unwrap_or_default();
        Ok(xml::proppatch_ok(&text))
    }
}

impl Vfs for WebDavFs {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn protocol(&self) -> Protocol {
        self.protocol
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::root())
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let ctx = dir.to_string();
        let url = self.client.url(dir.as_str(), true);
        let items = self.propfind(&url, "1", &ctx)?;
        let me = self.client.server_path(&url);
        // The folder itself is one of the responses: by its path, else the
        // first one when it is the parent of all others (a proxy that
        // rewrites the prefix).
        let own = items.iter().position(|i| i.path == me).or_else(|| {
            let first = &items.first()?.path;
            items[1..]
                .iter()
                .all(|i| i.path.starts_with(&format!("{first}/")))
                .then_some(0)
        });
        if let Some(i) = own {
            if !items[i].is_dir {
                return Err(VfsError::NotADirectory(ctx));
            }
        }
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            if Some(i) == own {
                continue;
            }
            let name = item.name();
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            out.push(WebDavFs::entry(&dir.join(name), item));
        }
        Ok(out)
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let item = self.stat_item(path)?;
        Ok(WebDavFs::entry(path, &item))
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let ctx = path.to_string();
        let url = self.client.url(path.as_str(), true);
        let resp = self.client.send_raw("MKCOL", &url, &[], Body::None, &ctx)?;
        match resp.status().as_u16() {
            200..=299 => Ok(()),
            // RFC 4918: 405 when something already exists there.
            405 => Err(VfsError::AlreadyExists(ctx)),
            s => Err(status_error(s, &ctx)),
        }
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        // DELETE on a folder removes everything below it: make sure.
        if self.stat_item(path)?.is_dir {
            return Err(VfsError::Other(format!("{path}: is a folder")));
        }
        let url = self.client.url(path.as_str(), false);
        self.client
            .send("DELETE", &url, &[], Body::None, path.as_str())?;
        Ok(())
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        if path.is_root() {
            return Err(VfsError::PermissionDenied(
                "/: the WebDAV root can't be removed".into(),
            ));
        }
        if !self.list(path)?.is_empty() {
            return Err(VfsError::DirectoryNotEmpty(path.to_string()));
        }
        let url = self.client.url(path.as_str(), true);
        self.client
            .send("DELETE", &url, &[], Body::None, path.as_str())?;
        Ok(())
    }

    /// One `DELETE` removes the whole folder on the server.
    fn remove_tree(&self, path: &VfsPath) -> VfsResult<()> {
        if path.is_root() {
            return Err(VfsError::PermissionDenied(
                "/: the WebDAV root can't be removed".into(),
            ));
        }
        let url = self.client.url(path.as_str(), true);
        self.client
            .send("DELETE", &url, &[], Body::None, path.as_str())?;
        Ok(())
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        if from.is_root() {
            return Err(VfsError::PermissionDenied(
                "/: the WebDAV root can't be renamed".into(),
            ));
        }
        let url = self.client.url(from.as_str(), false);
        let headers = [
            ("destination", self.client.url(to.as_str(), false)),
            ("overwrite", "T".to_string()),
        ];
        self.client
            .send("MOVE", &url, &headers, Body::None, from.as_str())?;
        Ok(())
    }

    fn server_copy(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        let url = self.client.url(from.as_str(), false);
        let headers = [
            ("destination", self.client.url(to.as_str(), false)),
            ("overwrite", "F".to_string()),
            ("depth", "infinity".to_string()),
        ];
        self.client
            .send("COPY", &url, &headers, Body::None, from.as_str())?;
        Ok(())
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let url = self.client.url(path.as_str(), false);
        let resp = self
            .client
            .send("GET", &url, &[], Body::None, path.as_str())?;
        Ok(Box::new(resp.into_body().into_reader()))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        Ok(Box::new(DavWriter {
            fs: WriterTarget {
                client: self.client.clone(),
                url: self.client.url(path.as_str(), false),
                path: path.clone(),
            },
            buf: Vec::new(),
            stream: None,
            done: false,
        }))
    }

    /// `PROPPATCH` of `getlastmodified` (Apache refuses it: a live
    /// property), and on Nextcloud / ownCloud their `lastmodified`.
    fn set_modified(&self, path: &VfsPath, mtime: DateTime<Utc>) -> VfsResult<()> {
        if self.no_mtime.load(Ordering::Relaxed) {
            return Err(VfsError::Unsupported("set_modified".into()));
        }
        if self.proppatch(path, "getlastmodified", &xml::http_date(mtime))? {
            return Ok(());
        }
        if matches!(self.preset.as_str(), "nextcloud" | "owncloud")
            && self.proppatch(path, "lastmodified", &mtime.timestamp().to_string())?
        {
            return Ok(());
        }
        self.no_mtime.store(true, Ordering::Relaxed);
        Err(VfsError::Unsupported("set_modified".into()))
    }

    /// Ranged GET (`Range: bytes=offset-`).
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        if offset == 0 {
            return self.open_read(path);
        }
        let url = self.client.url(path.as_str(), false);
        let resp = self.client.send(
            "GET",
            &url,
            &[("range", format!("bytes={offset}-"))],
            Body::None,
            path.as_str(),
        )?;
        let partial = resp.status().as_u16() == 206;
        let mut r = resp.into_body().into_reader();
        if !partial {
            // The server ignored the range: skip the head ourselves.
            let skipped = io::copy(&mut (&mut r).take(offset), &mut io::sink())?;
            if skipped < offset {
                return Err(VfsError::Other(format!(
                    "{path}: shorter than the resume offset {offset}"
                )));
            }
        }
        Ok(Box::new(r))
    }

    fn supports_resume(&self) -> ResumeCaps {
        ResumeCaps {
            read_at: true,
            append: false,
            upload_session: false,
        }
    }

    fn content_tag(&self, path: &VfsPath) -> Option<String> {
        self.stat_item(path).ok()?.etag
    }

    fn reconnect(&self) -> VfsResult<()> {
        self.client.reset_auth();
        Ok(())
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

struct WriterTarget {
    client: Arc<Client>,
    url: String,
    path: VfsPath,
}

/// A streamed PUT running on its own thread, fed through a channel.
struct Streaming {
    tx: Option<SyncSender<Result<Vec<u8>, ()>>>,
    handle: Option<JoinHandle<VfsResult<()>>>,
}

impl Streaming {
    fn join(&mut self) -> VfsResult<()> {
        self.tx = None;
        match self.handle.take() {
            Some(h) => h
                .join()
                .unwrap_or_else(|_| Err(VfsError::Other("upload thread panicked".into()))),
            None => Ok(()),
        }
    }
}

/// The request body of a streamed PUT: chunks from the writer; an `Err`
/// chunk aborts the request (cancelled copy), a closed channel ends it.
struct ChannelReader {
    rx: Receiver<Result<Vec<u8>, ()>>,
    chunk: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.chunk.len() {
            match self.rx.recv() {
                Ok(Ok(c)) => {
                    self.chunk = c;
                    self.pos = 0;
                }
                Ok(Err(())) => return Err(io::Error::other("upload cancelled")),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.pos);
        buf[..n].copy_from_slice(&self.chunk[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Upload writer (see the module docs). `flush()` completes the upload and
/// reports the server's verdict; dropping it unflushed abandons the upload.
struct DavWriter {
    fs: WriterTarget,
    buf: Vec<u8>,
    stream: Option<Streaming>,
    done: bool,
}

impl DavWriter {
    fn start_stream(&mut self) {
        let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, ()>>(4);
        let client = self.fs.client.clone();
        let url = self.fs.url.clone();
        let ctx = self.fs.path.to_string();
        let handle = std::thread::Builder::new()
            .name("webdav-put".into())
            .spawn(move || {
                let reader = ChannelReader {
                    rx,
                    chunk: Vec::new(),
                    pos: 0,
                };
                client
                    .send(
                        "PUT",
                        &url,
                        &[("content-type", "application/octet-stream".into())],
                        Body::Stream(Box::new(reader)),
                        &ctx,
                    )
                    .map(|_| ())
            });
        match handle {
            Ok(h) => {
                let first = std::mem::take(&mut self.buf);
                let _ = tx.send(Ok(first));
                self.stream = Some(Streaming {
                    tx: Some(tx),
                    handle: Some(h),
                });
            }
            Err(e) => {
                self.stream = Some(Streaming {
                    tx: None,
                    handle: Some(std::thread::spawn(move || {
                        Err(VfsError::Other(format!("cannot start the upload: {e}")))
                    })),
                });
            }
        }
    }

    fn fail(&mut self) -> io::Error {
        self.done = true;
        let err = match self.stream.as_mut() {
            Some(s) => s.join().err(),
            None => None,
        };
        io_err(err.unwrap_or_else(|| {
            VfsError::Connection(format!("{}: the upload stopped", self.fs.path))
        }))
    }
}

impl Write for DavWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.done {
            return Err(io::Error::other(format!(
                "{}: upload already completed",
                self.fs.path
            )));
        }
        if self.stream.is_none() {
            if self.buf.len() + data.len() <= BUFFERED_MAX {
                self.buf.extend_from_slice(data);
                return Ok(data.len());
            }
            self.start_stream();
        }
        let sent = self
            .stream
            .as_ref()
            .and_then(|s| s.tx.as_ref())
            .map(|tx| tx.send(Ok(data.to_vec())).is_ok())
            .unwrap_or(false);
        if sent {
            Ok(data.len())
        } else {
            Err(self.fail())
        }
    }

    /// Completes the upload. Later calls are no-ops.
    fn flush(&mut self) -> io::Result<()> {
        if self.done {
            return Ok(());
        }
        self.done = true;
        let r = match self.stream.as_mut() {
            Some(s) => s.join(),
            None => {
                let t = &self.fs;
                t.client
                    .send(
                        "PUT",
                        &t.url,
                        &[("content-type", "application/octet-stream".into())],
                        Body::Bytes(&self.buf),
                        t.path.as_str(),
                    )
                    .map(|_| ())
            }
        };
        self.buf = Vec::new();
        r.map_err(io_err)
    }
}

impl Drop for DavWriter {
    fn drop(&mut self) {
        // Not flushed: the copy was cancelled or failed. Abort a running
        // stream so the server never sees a complete (truncated) body.
        if !self.done {
            if let Some(s) = self.stream.as_mut() {
                if let Some(tx) = s.tx.take() {
                    let _ = tx.send(Err(()));
                }
                let _ = s.join();
            }
        }
    }
}

impl WebDavFs {
    /// Write a whole file in one request (tests, small files).
    pub fn put(&self, path: &VfsPath, data: &[u8]) -> VfsResult<()> {
        self.put_bytes(path, data)
    }
}
