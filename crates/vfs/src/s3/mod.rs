//! Amazon S3 and S3-compatible object storage (Cloudflare R2, MinIO,
//! Backblaze B2, Wasabi, DigitalOcean Spaces, Hetzner, ...).
//!
//! Path mapping: `/` lists the buckets (only the configured one when
//! [`S3Options::bucket`] is set), `/bucket/a/b` is key prefix `a/b` in
//! `bucket`. Folders are `CommonPrefixes` of a `delimiter=/` listing plus
//! zero-byte `prefix/` marker objects, which `create_dir` writes.
//!
//! Semantics worth knowing:
//! - `rename` is CopyObject + DeleteObject per key, so it is **not atomic**:
//!   an interrupted folder rename leaves keys in both places. Objects
//!   above 5 GiB can't be renamed (CopyObject limit) and are refused.
//! - `open_write` buffers up to [`PART_SIZE`] in memory; smaller files are
//!   sent with one PutObject, larger ones as a multipart upload. `flush()`
//!   completes the upload; dropping the writer without `flush()` aborts it.
//! - `set_modified` / `set_permissions` are unsupported (S3 sets
//!   LastModified itself and has no Unix permissions).
//! - `remove_tree` deletes everything under a prefix with batched
//!   DeleteObjects (1000 keys per request).

mod client;
mod creds;
mod sigv4;
mod xml;

use std::io::{self, Read, Write};
use std::sync::Arc;

use manta_core::{
    Auth, ConnectionConfig, Entry, EntryKind, Protocol, ResumableUpload, ResumeCaps, S3Options,
    UploadCheckpoint, UploadSession, Vfs, VfsError, VfsPath, VfsResult,
};

pub use client::Endpoint;
use client::{Client, Request};
pub use creds::list_profiles as aws_profiles;
use sigv4::Credentials;

/// Multipart part size (and the threshold above which multipart is used).
pub const PART_SIZE: usize = 16 * 1024 * 1024;
/// CopyObject's single-request limit.
const MAX_COPY_SIZE: u64 = 5 * 1024 * 1024 * 1024;
/// DeleteObjects accepts at most this many keys per request.
const DELETE_BATCH: usize = 1000;

/// A provider preset for the connect dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// Endpoint template; `{region}` and `{account}` are substituted.
    pub endpoint: &'static str,
    pub default_region: &'static str,
    pub path_style: bool,
    /// Cloudflare R2: the endpoint needs the account ID.
    pub needs_account: bool,
}

/// Presets offered by the connect dialog.
pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "aws",
        name: "Amazon S3",
        endpoint: "https://s3.{region}.amazonaws.com",
        default_region: "us-east-1",
        path_style: false,
        needs_account: false,
    },
    Provider {
        id: "r2",
        name: "Cloudflare R2",
        endpoint: "https://{account}.r2.cloudflarestorage.com",
        default_region: "auto",
        path_style: true,
        needs_account: true,
    },
    Provider {
        id: "minio",
        name: "MinIO",
        endpoint: "http://localhost:9000",
        default_region: "us-east-1",
        path_style: true,
        needs_account: false,
    },
    Provider {
        id: "b2",
        name: "Backblaze B2",
        endpoint: "https://s3.{region}.backblazeb2.com",
        default_region: "us-west-004",
        path_style: false,
        needs_account: false,
    },
    Provider {
        id: "wasabi",
        name: "Wasabi",
        endpoint: "https://s3.{region}.wasabisys.com",
        default_region: "us-east-1",
        path_style: false,
        needs_account: false,
    },
    Provider {
        id: "do",
        name: "DigitalOcean Spaces",
        endpoint: "https://{region}.digitaloceanspaces.com",
        default_region: "nyc3",
        path_style: false,
        needs_account: false,
    },
    Provider {
        id: "hetzner",
        name: "Hetzner Object Storage",
        endpoint: "https://{region}.your-objectstorage.com",
        default_region: "fsn1",
        path_style: false,
        needs_account: false,
    },
    Provider {
        id: "custom",
        name: "Other S3-compatible",
        endpoint: "",
        default_region: "us-east-1",
        path_style: true,
        needs_account: false,
    },
];

impl Provider {
    pub fn by_id(id: &str) -> Option<&'static Provider> {
        PROVIDERS.iter().find(|p| p.id == id)
    }

    /// The endpoint URL for `region` / `account`.
    pub fn endpoint_for(&self, region: &str, account: &str) -> String {
        let region = if region.trim().is_empty() {
            self.default_region
        } else {
            region.trim()
        };
        self.endpoint
            .replace("{region}", region)
            .replace("{account}", account.trim())
    }
}

/// An S3 connection.
pub struct S3Fs {
    label: String,
    client: Arc<Client>,
    /// The configured bucket, if the connection is limited to one.
    bucket: Option<String>,
}

fn not_dir(path: &VfsPath) -> VfsError {
    VfsError::NotADirectory(path.to_string())
}

impl S3Fs {
    /// Build the client from `cfg` and check the credentials with one
    /// request (ListBuckets, or a 1-key listing of the configured bucket).
    pub fn connect(cfg: &ConnectionConfig) -> VfsResult<S3Fs> {
        let fs = S3Fs::new(cfg)?;
        match &fs.bucket {
            Some(b) => {
                fs.list_page(b, "", Some("/"), None, Some(1), &format!("/{b}"))?;
            }
            None => {
                fs.list_buckets()?;
            }
        }
        Ok(fs)
    }

    /// Build the client without any network traffic.
    pub fn new(cfg: &ConnectionConfig) -> VfsResult<S3Fs> {
        let opts = cfg.s3.clone().unwrap_or_default();
        let (creds, profile_region, profile_endpoint) = resolve_credentials(cfg, &opts)?;
        let region = [
            opts.region.trim().to_string(),
            profile_region.unwrap_or_default(),
        ]
        .into_iter()
        .chain(std::env::var("AWS_REGION").ok())
        .find(|r| !r.is_empty())
        .unwrap_or_else(|| "us-east-1".into());
        let endpoint_str = [
            opts.endpoint.trim(),
            profile_endpoint.as_deref().unwrap_or(""),
            cfg.host.trim(),
        ]
        .into_iter()
        .find(|e| !e.is_empty())
        .map(str::to_string);
        let endpoint = match endpoint_str {
            Some(e) => Endpoint::parse(&e).map_err(VfsError::Connection)?,
            None => Endpoint::aws(&region),
        };
        let bucket =
            Some(opts.bucket.trim().trim_matches('/').to_string()).filter(|b| !b.is_empty());
        let label = if !cfg.name.trim().is_empty() {
            format!("{} (S3)", cfg.name.trim())
        } else if let Some(b) = &bucket {
            format!("{b} (S3)")
        } else {
            format!("{} (S3)", endpoint.hostname())
        };
        Ok(S3Fs {
            label,
            client: Arc::new(Client::new(endpoint, region, opts.path_style, creds)),
            bucket,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.client.endpoint
    }

    pub fn region(&self) -> &str {
        &self.client.region
    }

    /// `(bucket, key)` for a path; `None` for `/`.
    pub fn split(path: &VfsPath) -> Option<(String, String)> {
        let comps = path.components();
        let (first, rest) = comps.split_first()?;
        Some((first.to_string(), rest.join("/")))
    }

    /// `s3://bucket/key` for a path (as the AWS CLI writes it).
    pub fn s3_uri(path: &VfsPath) -> String {
        match S3Fs::split(path) {
            None => "s3://".into(),
            Some((b, k)) if k.is_empty() => format!("s3://{b}"),
            Some((b, k)) => format!("s3://{b}/{k}"),
        }
    }

    fn list_buckets(&self) -> VfsResult<Vec<xml::Bucket>> {
        let text = self.client.send_text(&Request::new("GET", None, ""), "/")?;
        xml::parse_list_buckets(&text).map_err(VfsError::Other)
    }

    fn list_page(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: Option<&str>,
        token: Option<&str>,
        max_keys: Option<u32>,
        ctx: &str,
    ) -> VfsResult<xml::ListPage> {
        let mut req = Request::new("GET", Some(bucket), "")
            .query("list-type", "2")
            .query("prefix", prefix);
        if let Some(d) = delimiter {
            req = req.query("delimiter", d);
        }
        if let Some(t) = token {
            req = req.query("continuation-token", t);
        }
        if let Some(m) = max_keys {
            req = req.query("max-keys", m.to_string());
        }
        let text = self.client.send_text(&req, ctx)?;
        xml::parse_list_objects(&text).map_err(|e| VfsError::Other(format!("{ctx}: {e}")))
    }

    /// Every object under `prefix` (recursive), following continuation tokens.
    fn list_all(&self, bucket: &str, prefix: &str, ctx: &str) -> VfsResult<Vec<xml::Object>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let page = self.list_page(bucket, prefix, None, token.as_deref(), None, ctx)?;
            out.extend(page.objects);
            match page.next_token {
                Some(t) => token = Some(t),
                None => return Ok(out),
            }
        }
    }

    fn head(
        &self,
        bucket: &str,
        key: &str,
        ctx: &str,
    ) -> VfsResult<(u64, Option<chrono::DateTime<chrono::Utc>>)> {
        let resp = self
            .client
            .send(&Request::new("HEAD", Some(bucket), key), ctx)?;
        let h = resp.headers();
        let size = h
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let modified = h
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .and_then(xml::parse_time);
        Ok((size, modified))
    }

    fn delete_object(&self, bucket: &str, key: &str, ctx: &str) -> VfsResult<()> {
        self.client
            .send(&Request::new("DELETE", Some(bucket), key), ctx)
            .map(drop)
    }

    /// Batch-delete `keys` (DeleteObjects, 1000 per request).
    fn delete_keys(&self, bucket: &str, keys: &[String], ctx: &str) -> VfsResult<()> {
        for chunk in keys.chunks(DELETE_BATCH) {
            let body = xml::delete_objects_body(chunk);
            let req = Request::new("POST", Some(bucket), "")
                .query("delete", "")
                .header("content-md5", client::content_md5(body.as_bytes()))
                .header("content-type", "application/xml")
                .body(body.as_bytes());
            let text = self.client.send_text(&req, ctx)?;
            let errors = xml::parse_delete_errors(&text).unwrap_or_default();
            if let Some((key, code, message)) = errors.first() {
                return Err(client::map_error(
                    0,
                    code,
                    &format!("{message} ({} keys failed)", errors.len()),
                    None,
                    &format!("/{bucket}/{key}"),
                ));
            }
        }
        Ok(())
    }

    /// Delete every object under the folder `path` (including its marker).
    /// Returns the number of deleted keys. On `/bucket` the bucket is
    /// emptied but kept.
    pub fn delete_prefix(&self, path: &VfsPath) -> VfsResult<usize> {
        let (bucket, key) =
            S3Fs::split(path).ok_or_else(|| VfsError::PermissionDenied("/".into()))?;
        let prefix = if key.is_empty() {
            String::new()
        } else {
            format!("{key}/")
        };
        let ctx = path.to_string();
        let mut keys: Vec<String> = self
            .list_all(&bucket, &prefix, &ctx)?
            .into_iter()
            .map(|o| o.key)
            .collect();
        if !prefix.is_empty() && !keys.contains(&prefix) {
            keys.push(prefix);
        }
        self.delete_keys(&bucket, &keys, &ctx)?;
        Ok(keys.len())
    }

    fn copy_object(
        &self,
        from: (&str, &str),
        to: (&str, &str),
        size: u64,
        ctx: &str,
    ) -> VfsResult<()> {
        if size > MAX_COPY_SIZE {
            return Err(VfsError::Unsupported(format!(
                "{ctx}: objects larger than 5 GiB can't be renamed on S3; copy and delete it instead"
            )));
        }
        let source = format!("/{}/{}", from.0, sigv4::uri_encode(from.1, true));
        let req = Request::new("PUT", Some(to.0), to.1).header("x-amz-copy-source", source);
        // CopyObject can fail with HTTP 200 and an <Error> body.
        let text = self.client.send_text(&req, ctx)?;
        if let Some(e) = xml::parse_error(&text) {
            return Err(client::map_error(200, &e.code, &e.message, None, ctx));
        }
        Ok(())
    }

    fn dir_entry(
        path: VfsPath,
        name: String,
        modified: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Entry {
        Entry {
            hidden: name.starts_with('.'),
            name,
            path,
            kind: EntryKind::Directory,
            size: 0,
            modified,
            permissions: None,
            link_target: None,
        }
    }

    fn file_entry(
        path: VfsPath,
        name: String,
        size: u64,
        modified: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Entry {
        Entry {
            hidden: name.starts_with('.'),
            name,
            path,
            kind: EntryKind::File,
            size,
            modified,
            permissions: None,
            link_target: None,
        }
    }

    /// Entries of a listing of `prefix` (which ends with `/` or is empty).
    fn entries_from(dir: &VfsPath, prefix: &str, page: &xml::ListPage, out: &mut Vec<Entry>) {
        for p in &page.prefixes {
            let name = p.strip_prefix(prefix).unwrap_or(p).trim_end_matches('/');
            if !name.is_empty()
                && !name.contains('/')
                && !out.iter().any(|e| e.is_dir() && e.name == name)
            {
                out.push(S3Fs::dir_entry(dir.join(name), name.to_string(), None));
            }
        }
        for o in &page.objects {
            let name = o.key.strip_prefix(prefix).unwrap_or(&o.key);
            if name.is_empty() || name.contains('/') {
                continue; // the folder's own marker, or an odd `a//b` key
            }
            out.push(S3Fs::file_entry(
                dir.join(name),
                name.to_string(),
                o.size,
                o.modified,
            ));
        }
    }
}

fn resolve_credentials(
    cfg: &ConnectionConfig,
    opts: &S3Options,
) -> VfsResult<(Option<Credentials>, Option<String>, Option<String>)> {
    if let Some(profile) = &opts.profile {
        let p = creds::load_profile(profile)?;
        return Ok((Some(p.credentials), p.region, p.endpoint));
    }
    match &cfg.auth {
        Auth::None => Ok((None, None, None)),
        Auth::Password(secret) => {
            if cfg.username.trim().is_empty() || secret.is_empty() {
                return Err(VfsError::Auth(
                    "enter the access key ID and the secret access key".into(),
                ));
            }
            Ok((
                Some(Credentials {
                    access_key: cfg.username.trim().to_string(),
                    secret_key: secret.clone(),
                    session_token: None,
                }),
                None,
                None,
            ))
        }
        _ => Err(VfsError::Auth(
            "S3 signs in with an access key or an AWS profile".into(),
        )),
    }
}

impl Vfs for S3Fs {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn protocol(&self) -> Protocol {
        Protocol::S3
    }

    fn home(&self) -> VfsResult<VfsPath> {
        Ok(match &self.bucket {
            Some(b) => VfsPath::root().join(b),
            None => VfsPath::root(),
        })
    }

    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        let Some((bucket, key)) = S3Fs::split(dir) else {
            if let Some(b) = &self.bucket {
                return Ok(vec![S3Fs::dir_entry(
                    VfsPath::root().join(b),
                    b.clone(),
                    None,
                )]);
            }
            return Ok(self
                .list_buckets()?
                .into_iter()
                .map(|b| S3Fs::dir_entry(VfsPath::root().join(&b.name), b.name, b.created))
                .collect());
        };
        let prefix = if key.is_empty() {
            String::new()
        } else {
            format!("{key}/")
        };
        let ctx = dir.to_string();
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        let mut any = false;
        loop {
            let page = self.list_page(&bucket, &prefix, Some("/"), token.as_deref(), None, &ctx)?;
            any |= !page.objects.is_empty() || !page.prefixes.is_empty();
            S3Fs::entries_from(dir, &prefix, &page, &mut out);
            match page.next_token {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        if !any && !key.is_empty() && self.head(&bucket, &key, &ctx).is_ok() {
            return Err(not_dir(dir));
        }
        Ok(out)
    }

    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        let ctx = path.to_string();
        let Some((bucket, key)) = S3Fs::split(path) else {
            return Ok(S3Fs::dir_entry(VfsPath::root(), String::new(), None));
        };
        if key.is_empty() {
            match self
                .client
                .send(&Request::new("HEAD", Some(&bucket), ""), &ctx)
            {
                // 403: the bucket exists, we just may not HEAD it.
                Ok(_) | Err(VfsError::PermissionDenied(_)) => {}
                Err(e) => return Err(e),
            }
            return Ok(S3Fs::dir_entry(path.clone(), bucket, None));
        }
        match self.head(&bucket, &key, &ctx) {
            Ok((size, modified)) => {
                return Ok(S3Fs::file_entry(
                    path.clone(),
                    path.file_name().to_string(),
                    size,
                    modified,
                ))
            }
            Err(VfsError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        let page = self.list_page(&bucket, &format!("{key}/"), Some("/"), None, Some(1), &ctx)?;
        if page.objects.is_empty() && page.prefixes.is_empty() {
            return Err(VfsError::NotFound(ctx));
        }
        let modified = page
            .objects
            .iter()
            .find(|o| o.key == format!("{key}/"))
            .and_then(|o| o.modified);
        Ok(S3Fs::dir_entry(
            path.clone(),
            path.file_name().to_string(),
            modified,
        ))
    }

    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let ctx = path.to_string();
        let (bucket, key) =
            S3Fs::split(path).ok_or_else(|| VfsError::AlreadyExists(ctx.clone()))?;
        if key.is_empty() {
            if self.bucket.is_some() {
                return Err(VfsError::Unsupported(
                    "this connection is limited to one bucket; new buckets can't be created here"
                        .into(),
                ));
            }
            let region = self.client.region.clone();
            let body = if region == "us-east-1" || region == "auto" || region.is_empty() {
                String::new()
            } else {
                xml::create_bucket_body(&region)
            };
            let req = Request::new("PUT", Some(&bucket), "").body(body.as_bytes());
            return self.client.send(&req, &ctx).map(drop);
        }
        if self.stat(path).is_ok() {
            return Err(VfsError::AlreadyExists(ctx));
        }
        let marker = format!("{key}/");
        self.client
            .send(&Request::new("PUT", Some(&bucket), &marker), &ctx)
            .map(drop)
    }

    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        let ctx = path.to_string();
        match S3Fs::split(path) {
            Some((bucket, key)) if !key.is_empty() => self.delete_object(&bucket, &key, &ctx),
            _ => Err(VfsError::Other(format!("{ctx}: is a bucket, not a file"))),
        }
    }

    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        let ctx = path.to_string();
        let (bucket, key) =
            S3Fs::split(path).ok_or_else(|| VfsError::PermissionDenied(ctx.clone()))?;
        if key.is_empty() {
            return self
                .client
                .send(&Request::new("DELETE", Some(&bucket), ""), &ctx)
                .map(drop);
        }
        let marker = format!("{key}/");
        let page = self.list_page(&bucket, &marker, None, None, Some(2), &ctx)?;
        if page.objects.iter().any(|o| o.key != marker) {
            return Err(VfsError::DirectoryNotEmpty(ctx));
        }
        self.delete_object(&bucket, &marker, &ctx)
    }

    fn remove_tree(&self, path: &VfsPath) -> VfsResult<()> {
        self.delete_prefix(path)?;
        if matches!(S3Fs::split(path), Some((_, k)) if k.is_empty()) {
            // A whole bucket: delete it too, now that it is empty.
            return self.remove_dir(path);
        }
        Ok(())
    }

    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        let ctx = from.to_string();
        let (Some((fb, fk)), Some((tb, tk))) = (S3Fs::split(from), S3Fs::split(to)) else {
            return Err(VfsError::Unsupported("buckets can't be renamed".into()));
        };
        if fk.is_empty() || tk.is_empty() {
            return Err(VfsError::Unsupported("buckets can't be renamed".into()));
        }
        let entry = self.stat(from)?;
        if !entry.is_dir() {
            self.copy_object((&fb, &fk), (&tb, &tk), entry.size, &ctx)?;
            return self.delete_object(&fb, &fk, &ctx);
        }
        // Folder: copy every key, then batch-delete the originals.
        let from_prefix = format!("{fk}/");
        let to_prefix = format!("{tk}/");
        if tb == fb && to_prefix.starts_with(&from_prefix) {
            return Err(VfsError::Other(format!(
                "{ctx}: cannot move a folder into itself"
            )));
        }
        let objects = self.list_all(&fb, &from_prefix, &ctx)?;
        if let Some(big) = objects.iter().find(|o| o.size > MAX_COPY_SIZE) {
            return Err(VfsError::Unsupported(format!(
                "/{fb}/{}: objects larger than 5 GiB can't be renamed on S3",
                big.key
            )));
        }
        for o in &objects {
            let rest = &o.key[from_prefix.len()..];
            self.copy_object(
                (&fb, &o.key),
                (&tb, &format!("{to_prefix}{rest}")),
                o.size,
                &ctx,
            )?;
        }
        if !objects.iter().any(|o| o.key == from_prefix) {
            // Keep the (possibly empty) folder visible at its new place.
            self.client
                .send(&Request::new("PUT", Some(&tb), &to_prefix), &ctx)?;
        }
        let keys: Vec<String> = objects.into_iter().map(|o| o.key).collect();
        self.delete_keys(&fb, &keys, &ctx)
    }

    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        let ctx = path.to_string();
        let (bucket, key) = S3Fs::split(path)
            .filter(|(_, k)| !k.is_empty())
            .ok_or_else(|| VfsError::Other(format!("{ctx}: is a bucket, not a file")))?;
        let resp = self
            .client
            .send(&Request::new("GET", Some(&bucket), &key), &ctx)?;
        Ok(Box::new(BodyReader(resp.into_body().into_reader())))
    }

    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        let ctx = path.to_string();
        let (bucket, key) = S3Fs::split(path)
            .filter(|(_, k)| !k.is_empty())
            .ok_or_else(|| {
                VfsError::Other(format!("{ctx}: files can't be stored outside a bucket"))
            })?;
        Ok(Box::new(S3Writer {
            client: self.client.clone(),
            bucket,
            key,
            ctx,
            buf: Vec::new(),
            upload_id: None,
            parts: Vec::new(),
            done: false,
            checkpoint: None,
        }))
    }

    /// Ranged GET (`Range: bytes=offset-`).
    fn open_read_at(&self, path: &VfsPath, offset: u64) -> VfsResult<Box<dyn Read + Send>> {
        if offset == 0 {
            return self.open_read(path);
        }
        let ctx = path.to_string();
        let (bucket, key) = S3Fs::split(path)
            .filter(|(_, k)| !k.is_empty())
            .ok_or_else(|| VfsError::Other(format!("{ctx}: is a bucket, not a file")))?;
        let req =
            Request::new("GET", Some(&bucket), &key).header("range", format!("bytes={offset}-"));
        let resp = self.client.send(&req, &ctx)?;
        if resp.status().as_u16() != 206 {
            // The server ignored the range: skip the head ourselves.
            let mut r = BodyReader(resp.into_body().into_reader());
            let skipped = io::copy(&mut (&mut r).take(offset), &mut io::sink())?;
            if skipped < offset {
                return Err(VfsError::Other(format!(
                    "{ctx}: shorter than the resume offset {offset}"
                )));
            }
            return Ok(Box::new(r));
        }
        Ok(Box::new(BodyReader(resp.into_body().into_reader())))
    }

    fn supports_resume(&self) -> ResumeCaps {
        ResumeCaps {
            read_at: true,
            append: false,
            upload_session: true,
        }
    }

    /// The object's ETag (HEAD).
    fn content_tag(&self, path: &VfsPath) -> Option<String> {
        let (bucket, key) = S3Fs::split(path).filter(|(_, k)| !k.is_empty())?;
        let resp = self
            .client
            .send(&Request::new("HEAD", Some(&bucket), &key), path.as_str())
            .ok()?;
        resp.headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    }

    /// Multipart upload that survives network drops. Files up to
    /// [`PART_SIZE`] are one PutObject and simply start over. Larger ones
    /// report the upload id and every completed part to `checkpoint`; a
    /// resume lists the parts on the server (`ListParts`) and continues
    /// after the last complete one.
    fn open_upload(
        &self,
        path: &VfsPath,
        _size: u64,
        resume: Option<&UploadSession>,
        checkpoint: UploadCheckpoint,
    ) -> VfsResult<ResumableUpload> {
        let ctx = path.to_string();
        let (bucket, key) = S3Fs::split(path)
            .filter(|(_, k)| !k.is_empty())
            .ok_or_else(|| {
                VfsError::Other(format!("{ctx}: files can't be stored outside a bucket"))
            })?;
        let mut writer = S3Writer {
            client: self.client.clone(),
            bucket,
            key,
            ctx,
            buf: Vec::new(),
            upload_id: None,
            parts: Vec::new(),
            done: false,
            checkpoint: Some(checkpoint),
        };
        let mut offset = 0;
        if let Some(state) = resume.and_then(S3Session::decode) {
            if state.bucket == writer.bucket
                && state.key == writer.key
                && state.part_size == PART_SIZE as u64
            {
                match self.list_parts(&state.bucket, &state.key, &state.upload_id, &writer.ctx) {
                    Ok(listed) => {
                        // Continue after the complete parts 1..n; a part the
                        // server lost or that differs ends the run.
                        let mut parts = Vec::new();
                        for (i, p) in listed.iter().enumerate() {
                            let known = state
                                .parts
                                .iter()
                                .find(|(n, _)| *n == p.number)
                                .map(|(_, e)| e);
                            let ok = p.number as usize == i + 1
                                && p.size == PART_SIZE as u64
                                && known.is_none_or(|e| e == &p.etag);
                            if !ok {
                                break;
                            }
                            parts.push((p.number, p.etag.clone()));
                        }
                        offset = parts.len() as u64 * PART_SIZE as u64;
                        log::info!(
                            "{}: continuing multipart upload at part {}",
                            writer.ctx,
                            parts.len() + 1
                        );
                        writer.upload_id = Some(state.upload_id);
                        writer.parts = parts;
                        writer.report();
                    }
                    Err(VfsError::NotFound(_)) => {
                        log::info!(
                            "{}: the multipart upload expired; starting over",
                            writer.ctx
                        );
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(ResumableUpload {
            writer: Box::new(writer),
            offset,
        })
    }

    fn abort_upload(&self, _path: &VfsPath, session: &UploadSession) -> VfsResult<()> {
        let Some(state) = S3Session::decode(session) else {
            return Ok(());
        };
        let req = Request::new("DELETE", Some(&state.bucket), &state.key)
            .query("uploadId", state.upload_id);
        match self.client.send(&req, &state.key) {
            Ok(_) | Err(VfsError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// What the transfer journal keeps of a multipart upload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct S3Session {
    bucket: String,
    key: String,
    upload_id: String,
    part_size: u64,
    /// (part number, ETag) of the completed parts.
    parts: Vec<(u32, String)>,
}

impl S3Session {
    const KIND: &'static str = "s3-multipart";

    fn decode(s: &UploadSession) -> Option<S3Session> {
        (s.kind == Self::KIND)
            .then(|| serde_json::from_str(&s.data).ok())
            .flatten()
    }

    fn encode(&self) -> UploadSession {
        UploadSession {
            kind: Self::KIND.into(),
            data: serde_json::to_string(self).unwrap_or_default(),
            committed: self.parts.len() as u64 * self.part_size,
        }
    }
}

impl S3Fs {
    /// All parts of a multipart upload (ListParts, paginated).
    fn list_parts(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        ctx: &str,
    ) -> VfsResult<Vec<xml::Part>> {
        let mut out = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let mut req = Request::new("GET", Some(bucket), key).query("uploadId", upload_id);
            if let Some(m) = &marker {
                req = req.query("part-number-marker", m.clone());
            }
            let text = self.client.send_text(&req, ctx)?;
            let (parts, next) = xml::parse_list_parts(&text).map_err(VfsError::Other)?;
            out.extend(parts);
            match next {
                Some(m) if Some(&m) != marker.as_ref() => marker = Some(m),
                _ => break,
            }
        }
        out.sort_by_key(|p| p.number);
        Ok(out)
    }
}

/// Response body reader; maps transport errors to `io::Error`.
struct BodyReader(ureq::BodyReader<'static>);

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

/// Upload writer: PutObject for small files, multipart above [`PART_SIZE`].
struct S3Writer {
    client: Arc<Client>,
    bucket: String,
    key: String,
    ctx: String,
    buf: Vec<u8>,
    upload_id: Option<String>,
    /// (part number, ETag) of uploaded parts.
    parts: Vec<(u32, String)>,
    done: bool,
    /// Resumable upload ([`Vfs::open_upload`]): progress goes here, and a
    /// failed or dropped upload is kept on the server for a later resume.
    checkpoint: Option<UploadCheckpoint>,
}

fn io_err(e: VfsError) -> io::Error {
    match e {
        VfsError::Io(e) => e,
        VfsError::NotFound(m) => io::Error::new(io::ErrorKind::NotFound, m),
        VfsError::PermissionDenied(m) => io::Error::new(io::ErrorKind::PermissionDenied, m),
        other => io::Error::other(other.to_string()),
    }
}

impl S3Writer {
    /// Tell the resumable-upload caller where we stand.
    fn report(&self) {
        if let (Some(cp), Some(id)) = (&self.checkpoint, &self.upload_id) {
            cp(&S3Session {
                bucket: self.bucket.clone(),
                key: self.key.clone(),
                upload_id: id.clone(),
                part_size: PART_SIZE as u64,
                parts: self.parts.clone(),
            }
            .encode());
        }
    }

    fn upload_part(&mut self) -> VfsResult<()> {
        if self.upload_id.is_none() {
            let req = Request::new("POST", Some(&self.bucket), &self.key).query("uploads", "");
            let text = self.client.send_text(&req, &self.ctx)?;
            self.upload_id = Some(xml::parse_upload_id(&text).map_err(VfsError::Other)?);
            self.report();
        }
        let n = self.parts.len() as u32 + 1;
        let id = self.upload_id.clone().unwrap_or_default();
        let req = Request::new("PUT", Some(&self.bucket), &self.key)
            .query("partNumber", n.to_string())
            .query("uploadId", id)
            .body(&self.buf);
        let resp = self.client.send(&req, &self.ctx)?;
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| VfsError::Other(format!("{}: part {n} has no ETag", self.ctx)))?
            .to_string();
        self.parts.push((n, etag));
        self.buf.clear();
        self.report();
        Ok(())
    }

    fn finish(&mut self) -> VfsResult<()> {
        let Some(id) = self.upload_id.clone() else {
            let req = Request::new("PUT", Some(&self.bucket), &self.key).body(&self.buf);
            self.client.send(&req, &self.ctx)?;
            self.buf = Vec::new();
            return Ok(());
        };
        if !self.buf.is_empty() {
            self.upload_part()?;
        }
        let body = xml::complete_multipart_body(&self.parts);
        let req = Request::new("POST", Some(&self.bucket), &self.key)
            .query("uploadId", id)
            .header("content-type", "application/xml")
            .body(body.as_bytes());
        // CompleteMultipartUpload can fail with HTTP 200 and an <Error> body.
        let text = self.client.send_text(&req, &self.ctx)?;
        if let Some(e) = xml::parse_error(&text) {
            return Err(client::map_error(200, &e.code, &e.message, None, &self.ctx));
        }
        self.upload_id = None;
        Ok(())
    }

    fn abort(&mut self) {
        if let Some(id) = self.upload_id.take() {
            let req = Request::new("DELETE", Some(&self.bucket), &self.key).query("uploadId", id);
            if let Err(e) = self.client.send(&req, &self.ctx) {
                log::warn!("aborting multipart upload of {}: {e}", self.ctx);
            }
        }
    }
}

impl Write for S3Writer {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.done {
            return Err(io::Error::other(format!(
                "{}: upload already completed",
                self.ctx
            )));
        }
        let room = PART_SIZE - self.buf.len();
        let n = data.len().min(room);
        self.buf.extend_from_slice(&data[..n]);
        if self.buf.len() >= PART_SIZE {
            if let Err(e) = self.upload_part() {
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
        let r = self.finish();
        if r.is_err() && self.checkpoint.is_none() {
            self.abort();
        }
        r.map_err(io_err)
    }
}

impl Drop for S3Writer {
    fn drop(&mut self) {
        // Not flushed: the copy was cancelled or failed. Nothing reaches the
        // bucket; an open multipart upload is aborted so its parts don't
        // linger (and cost money).
        if !self.done && self.checkpoint.is_none() {
            self.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mapping() {
        assert_eq!(S3Fs::split(&VfsPath::root()), None);
        assert_eq!(
            S3Fs::split(&VfsPath::new("/photos")),
            Some(("photos".into(), String::new()))
        );
        assert_eq!(
            S3Fs::split(&VfsPath::new("/photos/2024/a b.jpg")),
            Some(("photos".into(), "2024/a b.jpg".into()))
        );
        assert_eq!(
            S3Fs::s3_uri(&VfsPath::new("/photos/2024")),
            "s3://photos/2024"
        );
        assert_eq!(S3Fs::s3_uri(&VfsPath::new("/photos")), "s3://photos");
    }

    #[test]
    fn listing_to_entries() {
        let page = xml::ListPage {
            prefixes: vec!["docs/raw/".into(), "docs/.cache/".into(), "docs//".into()],
            objects: vec![
                xml::Object {
                    key: "docs/".into(),
                    size: 0,
                    modified: None,
                },
                xml::Object {
                    key: "docs/readme.md".into(),
                    size: 12,
                    modified: None,
                },
            ],
            next_token: None,
        };
        let dir = VfsPath::new("/b/docs");
        let mut out = Vec::new();
        S3Fs::entries_from(&dir, "docs/", &page, &mut out);
        let names: Vec<_> = out
            .iter()
            .map(|e| (e.name.as_str(), e.is_dir(), e.hidden))
            .collect();
        assert_eq!(
            names,
            [
                ("raw", true, false),
                (".cache", true, true),
                ("readme.md", false, false)
            ]
        );
        assert_eq!(out[2].path, VfsPath::new("/b/docs/readme.md"));
        assert_eq!(out[2].size, 12);
    }

    #[test]
    fn providers() {
        let r2 = Provider::by_id("r2").unwrap();
        assert_eq!(
            r2.endpoint_for("", "abc123"),
            "https://abc123.r2.cloudflarestorage.com"
        );
        assert_eq!(
            Provider::by_id("aws")
                .unwrap()
                .endpoint_for("eu-central-1", ""),
            "https://s3.eu-central-1.amazonaws.com"
        );
        assert_eq!(
            Provider::by_id("hetzner").unwrap().endpoint_for("", ""),
            "https://fsn1.your-objectstorage.com"
        );
    }

    fn cfg(opts: S3Options, user: &str, auth: Auth) -> ConnectionConfig {
        ConnectionConfig {
            name: String::new(),
            protocol: Protocol::S3,
            host: String::new(),
            port: 0,
            username: user.into(),
            auth,
            initial_path: None,
            s3: Some(Box::new(opts)),
            container: None,
            ftp: None,
        }
    }

    #[test]
    fn config_resolution() {
        let fs = S3Fs::new(&cfg(
            S3Options {
                region: "eu-west-1".into(),
                bucket: "/photos/".into(),
                ..Default::default()
            },
            "AKIA",
            Auth::Password("s".into()),
        ))
        .unwrap();
        assert_eq!(fs.endpoint().host, "s3.eu-west-1.amazonaws.com");
        assert_eq!(fs.home().unwrap(), VfsPath::new("/photos"));
        assert_eq!(fs.label(), "photos (S3)");
        let fs = S3Fs::new(&cfg(
            S3Options {
                endpoint: "http://127.0.0.1:9000".into(),
                path_style: true,
                ..Default::default()
            },
            "minio",
            Auth::Password("minio123".into()),
        ))
        .unwrap();
        assert_eq!(fs.home().unwrap(), VfsPath::root());
        assert!(!fs.endpoint().https);
        assert!(matches!(
            S3Fs::new(&cfg(
                S3Options::default(),
                "",
                Auth::Password(String::new())
            )),
            Err(VfsError::Auth(_))
        ));
    }
}
