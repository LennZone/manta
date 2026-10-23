//! Streaming upload with Drive's resumable protocol: the session starts with
//! the first full chunk (or at `flush`), chunks of [`super::UPLOAD_CHUNK`]
//! bytes follow with `Content-Range: bytes a-b/*`, the last one names the
//! total. The file only appears on Drive when `flush()` completes the
//! session; dropping an unflushed writer cancels the session, so a
//! cancelled copy leaves nothing behind.
//!
//! Resumable uploads ([`manta_core::Vfs::open_upload`]) keep the session
//! instead: its URI and confirmed offset go to the checkpoint, and a later
//! run asks Drive how far it got (`Content-Range: bytes */size`) and
//! continues there.

use std::io::{self, Write};
use std::sync::Arc;

use manta_core::{UploadCheckpoint, UploadSession, VfsError, VfsPath, VfsResult};
use serde_json::json;

use super::api::Call;
use super::Inner;

pub(crate) enum Target {
    Create { parent_id: String, name: String },
    Update { id: String },
}

pub(crate) struct DriveWriter {
    inner: Arc<Inner>,
    target: Target,
    ctx: String,
    buf: Vec<u8>,
    session: Option<String>,
    /// Bytes the server has confirmed.
    offset: u64,
    done: bool,
    /// Resumable: report progress here, keep the session on errors / drop.
    checkpoint: Option<UploadCheckpoint>,
}

/// What the transfer journal keeps of a Drive upload session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DriveSession {
    pub uri: String,
}

impl DriveSession {
    pub const KIND: &'static str = "gdrive-resumable";

    pub fn decode(s: &UploadSession) -> Option<DriveSession> {
        (s.kind == Self::KIND)
            .then(|| serde_json::from_str(&s.data).ok())
            .flatten()
    }

    fn encode(&self, committed: u64) -> UploadSession {
        UploadSession {
            kind: Self::KIND.into(),
            data: serde_json::to_string(self).unwrap_or_default(),
            committed,
        }
    }
}

/// Where a resumable session stands on the server.
pub(crate) enum SessionState {
    /// Bytes confirmed so far.
    Partial(u64),
    /// The upload already completed.
    Complete,
}

fn io_err(e: VfsError) -> io::Error {
    match e {
        VfsError::Io(e) => e,
        VfsError::NotFound(m) => io::Error::new(io::ErrorKind::NotFound, m),
        VfsError::PermissionDenied(m) => io::Error::new(io::ErrorKind::PermissionDenied, m),
        other => io::Error::other(other.to_string()),
    }
}

/// Upper end of a `Range: bytes=0-N` header (exclusive), 0 if absent.
pub(crate) fn confirmed_bytes(range: Option<&str>) -> u64 {
    range
        .and_then(|r| r.trim().strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
        .and_then(|(_, end)| end.trim().parse::<u64>().ok())
        .map_or(0, |end| end + 1)
}

impl DriveWriter {
    pub(crate) fn new(inner: Arc<Inner>, target: Target, ctx: String) -> DriveWriter {
        DriveWriter {
            inner,
            target,
            ctx,
            buf: Vec::new(),
            session: None,
            offset: 0,
            done: false,
            checkpoint: None,
        }
    }

    /// A resumable writer (see the module docs).
    pub(crate) fn resumable(
        inner: Arc<Inner>,
        target: Target,
        ctx: String,
        checkpoint: UploadCheckpoint,
    ) -> DriveWriter {
        let mut w = DriveWriter::new(inner, target, ctx);
        w.checkpoint = Some(checkpoint);
        w
    }

    /// Continue `uri` at `offset` (from [`Self::query`]).
    pub(crate) fn continue_at(&mut self, uri: String, offset: u64) {
        self.session = Some(uri);
        self.offset = offset;
        self.report();
    }

    /// The session completed already: nothing left to send.
    pub(crate) fn completed(&mut self) {
        self.done = true;
        let parent = VfsPath::new(&self.ctx)
            .parent()
            .unwrap_or_else(VfsPath::root);
        self.inner.forget(&parent);
    }

    fn report(&self) {
        if let (Some(cp), Some(uri)) = (&self.checkpoint, &self.session) {
            cp(&DriveSession { uri: uri.clone() }.encode(self.offset));
        }
    }

    /// Ask Drive how much of the session `uri` it has (`bytes */size`).
    pub(crate) fn query(inner: &Inner, uri: &str, size: u64, ctx: &str) -> VfsResult<SessionState> {
        let mut call =
            Call::new("PUT", uri.to_string()).header("content-range", format!("bytes */{size}"));
        call.accept_308 = true;
        let resp = inner.api.send(&call, ctx)?;
        if resp.status().as_u16() == 308 {
            Ok(SessionState::Partial(confirmed_bytes(
                resp.headers().get("range").and_then(|v| v.to_str().ok()),
            )))
        } else {
            Ok(SessionState::Complete)
        }
    }

    fn start_session(&self) -> VfsResult<String> {
        let base = format!("{}/upload/drive/v3/files", self.inner.api.base());
        let call = match &self.target {
            Target::Create { parent_id, name } => {
                Call::new("POST", base).json(&json!({ "name": name, "parents": [parent_id] }))
            }
            Target::Update { id } => Call::new("PATCH", format!("{base}/{id}")).json(&json!({})),
        }
        .query("uploadType", "resumable")
        .query("supportsAllDrives", "true");
        let resp = self.inner.api.send(&call, &self.ctx)?;
        resp.headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| VfsError::Other(format!("{}: Drive did not start the upload", self.ctx)))
    }

    /// Send the buffer; `last` completes the upload.
    fn send_chunk(&mut self, last: bool) -> VfsResult<()> {
        if self.session.is_none() {
            self.session = Some(self.start_session()?);
            self.report();
        }
        let session = self.session.clone().unwrap_or_default();
        let mut stalls = 0;
        loop {
            let len = self.buf.len() as u64;
            let range = match (last, len) {
                (true, 0) => format!("bytes */{}", self.offset),
                (true, _) => format!(
                    "bytes {}-{}/{}",
                    self.offset,
                    self.offset + len - 1,
                    self.offset + len
                ),
                (false, _) => format!("bytes {}-{}/*", self.offset, self.offset + len - 1),
            };
            let mut call = Call::new("PUT", session.clone())
                .header("content-range", range)
                .bytes(&self.buf);
            call.accept_308 = true;
            let resp = self.inner.api.send(&call, &self.ctx)?;
            drop(call);
            if resp.status().as_u16() != 308 {
                // 200 / 201: complete.
                self.offset += len;
                self.buf.clear();
                return Ok(());
            }
            let confirmed =
                confirmed_bytes(resp.headers().get("range").and_then(|v| v.to_str().ok()));
            if confirmed < self.offset || confirmed > self.offset + len {
                return Err(VfsError::Other(format!(
                    "{}: Drive lost track of the upload",
                    self.ctx
                )));
            }
            let accepted = (confirmed - self.offset) as usize;
            self.buf.drain(..accepted);
            self.offset = confirmed;
            self.report();
            if !last {
                return Ok(());
            }
            stalls = if accepted == 0 { stalls + 1 } else { 0 };
            if stalls >= 3 {
                return Err(VfsError::Connection(format!(
                    "{}: the upload stopped making progress",
                    self.ctx
                )));
            }
        }
    }

    fn abort(&mut self) {
        if let Some(session) = self.session.take() {
            let mut call = Call::new("DELETE", session);
            call.no_retry = true;
            // Drive answers 499 to a cancelled session; nothing to report.
            let _ = self.inner.api.send(&call, &self.ctx);
        }
    }
}

impl Write for DriveWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.done {
            return Err(io::Error::other(format!(
                "{}: upload already completed",
                self.ctx
            )));
        }
        let chunk = self.inner.chunk_size;
        let n = data.len().min(chunk - self.buf.len().min(chunk));
        self.buf.extend_from_slice(&data[..n]);
        if self.buf.len() >= chunk {
            if let Err(e) = self.send_chunk(false) {
                if self.checkpoint.is_none() {
                    self.abort();
                }
                self.done = true;
                return Err(io_err(e));
            }
        }
        Ok(n)
    }

    /// Completes the upload. Later calls are no-ops.
    fn flush(&mut self) -> io::Result<()> {
        if self.done {
            return Ok(());
        }
        self.done = true;
        let r = self.send_chunk(true);
        if r.is_err() {
            if self.checkpoint.is_none() {
                self.abort();
            }
        } else {
            self.session = None;
            let parent = VfsPath::new(&self.ctx)
                .parent()
                .unwrap_or_else(VfsPath::root);
            self.inner.forget(&parent);
        }
        r.map_err(io_err)
    }
}

impl Drop for DriveWriter {
    fn drop(&mut self) {
        if !self.done && self.checkpoint.is_none() {
            self.abort();
        }
    }
}

/// Cancel a session by URI (`Vfs::abort_upload`).
pub(crate) fn abort_session(inner: &Inner, uri: &str, ctx: &str) {
    let mut call = Call::new("DELETE", uri.to_string());
    call.no_retry = true;
    let _ = inner.api.send(&call, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_header() {
        assert_eq!(confirmed_bytes(Some("bytes=0-262143")), 262_144);
        assert_eq!(confirmed_bytes(None), 0);
        assert_eq!(confirmed_bytes(Some("garbage")), 0);
    }
}
