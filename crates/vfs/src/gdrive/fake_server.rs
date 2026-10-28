//! A tiny fake of Google's OAuth token endpoint and the Drive API v3 on
//! 127.0.0.1, just enough for the backend: files.list (paging, `q` with
//! `'<id>' in parents` / `sharedWithMe`), drives.list, files.get (metadata,
//! `alt=media`), files.export, files.create (folders), files.update (rename,
//! move, trash, modifiedTime), resumable uploads, about.get. Knobs for
//! failure modes: expired access tokens, rate limiting, short upload acks.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::oauth::{parse_query, pkce_challenge};

pub(crate) const EMAIL: &str = "tester@example.com";
pub(crate) const REFRESH: &str = "1//fake-refresh";
pub(crate) const CODE: &str = "fake-auth-code";
pub(crate) const ROOT_ID: &str = "ROOT";

#[derive(Debug, Clone)]
pub(crate) struct FFile {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub parents: Vec<String>,
    pub data: Vec<u8>,
    pub created: String,
    pub modified: String,
    pub trashed: bool,
    pub shared_with_me: bool,
    pub drive_id: Option<String>,
}

#[derive(Debug)]
struct Upload {
    /// `Some(id)`: update that file; `None`: create from `meta`.
    update: Option<String>,
    meta: Value,
    data: Vec<u8>,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub files: BTreeMap<String, FFile>,
    pub drives: Vec<(String, String)>,
    next_id: u64,
    pub access_token: String,
    pub refreshes: usize,
    /// Answer the next N files.list calls with 403 userRateLimitExceeded.
    pub rate_limit_next: usize,
    /// Answer the next N requests with 503.
    pub fail_503_next: usize,
    /// Max files per page.
    pub page_cap: usize,
    pub list_calls: usize,
    /// Accept only half of the next non-final upload chunk.
    pub short_ack_once: bool,
    uploads: HashMap<String, Upload>,
    /// Content-Range headers of upload PUTs.
    pub chunk_ranges: Vec<String>,
    pub cancelled_uploads: usize,
    /// Downloads that asked for a byte range.
    pub ranged_gets: usize,
    /// PKCE: the challenge and redirect of the pending authorization.
    pub auth_challenge: String,
    pub auth_redirect: String,
}

impl State {
    pub fn add(&mut self, name: &str, mime: &str, parent: &str, data: &[u8]) -> String {
        self.next_id += 1;
        let id = format!("f{}", self.next_id);
        let created = format!("2024-01-01T00:00:{:02}.000Z", self.next_id % 60);
        let drive_id = self
            .files
            .get(parent)
            .and_then(|p| p.drive_id.clone())
            .or_else(|| {
                self.drives
                    .iter()
                    .find(|(d, _)| d == parent)
                    .map(|(d, _)| d.clone())
            });
        self.files.insert(
            id.clone(),
            FFile {
                id: id.clone(),
                name: name.into(),
                mime: mime.into(),
                parents: vec![parent.to_string()],
                data: data.to_vec(),
                created: created.clone(),
                modified: created,
                trashed: false,
                shared_with_me: false,
                drive_id,
            },
        );
        id
    }

    pub fn by_name(&self, name: &str) -> Vec<&FFile> {
        self.files.values().filter(|f| f.name == name).collect()
    }

    fn json(f: &FFile) -> Value {
        let mut v = json!({
            "id": f.id, "name": f.name, "mimeType": f.mime, "parents": f.parents,
            "createdTime": f.created, "modifiedTime": f.modified,
        });
        if !f.mime.starts_with("application/vnd.google-apps.") {
            v["size"] = json!(f.data.len().to_string());
        }
        if let Some(d) = &f.drive_id {
            v["driveId"] = json!(d);
        }
        v
    }
}

pub(crate) struct FakeDrive {
    pub port: u16,
    pub state: Arc<Mutex<State>>,
}

impl FakeDrive {
    pub fn start() -> FakeDrive {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let state = Arc::new(Mutex::new(State {
            access_token: "access-0".into(),
            page_cap: 1000,
            ..State::default()
        }));
        let st = state.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let st = st.clone();
                thread::spawn(move || handle(stream, &st, port));
            }
        });
        FakeDrive { port, state }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

struct Req {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Req {
    fn q(&self, k: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
}

fn read_req(stream: &mut TcpStream) -> Option<Req> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let mut data = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    let head_end = loop {
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = data[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (target, Vec::new()),
    };
    Some(Req {
        method,
        path,
        query,
        headers,
        body,
    })
}

struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn resp_json(status: u16, v: Value) -> Resp {
    Resp {
        status,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: v.to_string().into_bytes(),
    }
}

fn drive_err(status: u16, reason: &str, msg: &str) -> Resp {
    resp_json(
        status,
        json!({"error": {"code": status, "message": msg, "errors": [{"reason": reason, "message": msg}]}}),
    )
}

fn handle(mut stream: TcpStream, state: &Arc<Mutex<State>>, port: u16) {
    let Some(req) = read_req(&mut stream) else {
        return;
    };
    let resp = route(&req, state, port);
    let reason = match resp.status {
        200 => "OK",
        308 => "Resume Incomplete",
        _ => "X",
    };
    let mut out = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        resp.body.len()
    );
    for (k, v) in &resp.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.write_all(&resp.body);
    let _ = stream.flush();
}

fn route(req: &Req, state: &Arc<Mutex<State>>, port: u16) -> Resp {
    let mut st = state.lock().unwrap();
    if req.path == "/token" {
        return token(req, &mut st);
    }
    if st.fail_503_next > 0 {
        st.fail_503_next -= 1;
        return drive_err(503, "backendError", "Backend Error");
    }
    let auth = req
        .headers
        .get("authorization")
        .cloned()
        .unwrap_or_default();
    if auth != format!("Bearer {}", st.access_token) {
        return drive_err(401, "authError", "Invalid Credentials");
    }
    let segs: Vec<&str> = req.path.trim_start_matches('/').split('/').collect();
    match (req.method.as_str(), segs.as_slice()) {
        ("GET", ["drive", "v3", "about"]) => resp_json(
            200,
            json!({
                "user": {"emailAddress": EMAIL, "displayName": "Test User"},
                "storageQuota": {"limit": "16106127360", "usage": "5368709120"}
            }),
        ),
        ("GET", ["drive", "v3", "drives"]) => {
            let drives: Vec<Value> = st
                .drives
                .iter()
                .map(|(id, name)| json!({"id": id, "name": name}))
                .collect();
            resp_json(200, json!({ "drives": drives }))
        }
        ("GET", ["drive", "v3", "files"]) => list(req, &mut st),
        ("GET", ["drive", "v3", "files", id]) => {
            let Some(f) = st.files.get(*id) else {
                return drive_err(404, "notFound", "File not found");
            };
            if req.q("alt") == Some("media") {
                if f.mime.starts_with("application/vnd.google-apps.") {
                    return drive_err(
                        403,
                        "fileNotDownloadable",
                        "Only files with binary content can be downloaded",
                    );
                }
                // `Range: bytes=N-` (resumed downloads).
                if let Some(start) = req
                    .headers
                    .get("range")
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.strip_suffix('-'))
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    let body = f.data.get(start..).unwrap_or_default().to_vec();
                    st.ranged_gets += 1;
                    return Resp {
                        status: 206,
                        headers: vec![],
                        body,
                    };
                }
                return Resp {
                    status: 200,
                    headers: vec![],
                    body: f.data.clone(),
                };
            }
            resp_json(200, State::json(f))
        }
        ("GET", ["drive", "v3", "files", id, "export"]) => {
            let Some(f) = st.files.get(*id) else {
                return drive_err(404, "notFound", "File not found");
            };
            if !f.mime.starts_with("application/vnd.google-apps.") {
                return drive_err(
                    403,
                    "fileNotExportable",
                    "Export only supports Docs Editors files",
                );
            }
            let body =
                format!("EXPORT {} AS {}", f.name, req.q("mimeType").unwrap_or("")).into_bytes();
            Resp {
                status: 200,
                headers: vec![],
                body,
            }
        }
        ("POST", ["drive", "v3", "files"]) => {
            let meta: Value = serde_json::from_slice(&req.body).unwrap_or_default();
            let parent = meta["parents"][0].as_str().unwrap_or("root");
            let parent = if parent == "root" { ROOT_ID } else { parent }.to_string();
            let id = st.add(
                meta["name"].as_str().unwrap_or(""),
                meta["mimeType"].as_str().unwrap_or(""),
                &parent,
                b"",
            );
            resp_json(200, json!({ "id": id }))
        }
        ("PATCH", ["drive", "v3", "files", id]) => {
            let meta: Value = serde_json::from_slice(&req.body).unwrap_or_default();
            let add = req.q("addParents").map(|p| {
                if p == "root" {
                    ROOT_ID.to_string()
                } else {
                    p.to_string()
                }
            });
            let remove: Vec<String> = req
                .q("removeParents")
                .map(|r| r.split(',').map(str::to_string).collect())
                .unwrap_or_default();
            let Some(f) = st.files.get_mut(*id) else {
                return drive_err(404, "notFound", "File not found");
            };
            if let Some(n) = meta["name"].as_str() {
                f.name = n.into();
            }
            if let Some(t) = meta["trashed"].as_bool() {
                f.trashed = t;
            }
            if let Some(m) = meta["modifiedTime"].as_str() {
                f.modified = m.into();
            }
            if let Some(a) = add {
                f.parents.retain(|p| !remove.contains(p));
                f.parents.push(a);
            }
            let v = State::json(f);
            resp_json(200, v)
        }
        ("POST", ["upload", "drive", "v3", "files"])
        | ("PATCH", ["upload", "drive", "v3", "files", _]) => {
            if req.q("uploadType") != Some("resumable") {
                return drive_err(400, "badRequest", "uploadType");
            }
            let update = segs.get(4).map(|s| s.to_string());
            if let Some(id) = &update {
                if !st.files.contains_key(id) {
                    return drive_err(404, "notFound", "File not found");
                }
            }
            st.next_id += 1;
            let sid = format!("s{}", st.next_id);
            let meta: Value = serde_json::from_slice(&req.body).unwrap_or_default();
            st.uploads.insert(
                sid.clone(),
                Upload {
                    update,
                    meta,
                    data: Vec::new(),
                },
            );
            Resp {
                status: 200,
                headers: vec![(
                    "Location".into(),
                    format!("http://127.0.0.1:{port}/upload/session/{sid}"),
                )],
                body: Vec::new(),
            }
        }
        ("PUT", ["upload", "session", sid]) => upload_chunk(req, &mut st, sid),
        ("DELETE", ["upload", "session", sid]) => {
            if st.uploads.remove(*sid).is_some() {
                st.cancelled_uploads += 1;
            }
            Resp {
                status: 499,
                headers: vec![],
                body: Vec::new(),
            }
        }
        _ => drive_err(
            404,
            "notFound",
            &format!("no route {} {}", req.method, req.path),
        ),
    }
}

fn token(req: &Req, st: &mut State) -> Resp {
    let form = parse_query(&String::from_utf8_lossy(&req.body));
    let get = |k: &str| {
        form.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    if get("client_id") != "test-client" {
        return resp_json(
            401,
            json!({"error": "invalid_client", "error_description": "The OAuth client was not found."}),
        );
    }
    match get("grant_type").as_str() {
        "refresh_token" if get("refresh_token") == REFRESH => {
            st.refreshes += 1;
            st.access_token = format!("access-{}", st.refreshes);
            resp_json(
                200,
                json!({"access_token": st.access_token, "expires_in": 3599, "token_type": "Bearer"}),
            )
        }
        "authorization_code"
            if get("code") == CODE
                && pkce_challenge(&get("code_verifier")) == st.auth_challenge
                && get("redirect_uri") == st.auth_redirect =>
        {
            st.refreshes += 1;
            st.access_token = format!("access-{}", st.refreshes);
            resp_json(
                200,
                json!({"access_token": st.access_token, "refresh_token": REFRESH, "expires_in": 3599, "token_type": "Bearer"}),
            )
        }
        _ => resp_json(
            400,
            json!({"error": "invalid_grant", "error_description": "Bad Request"}),
        ),
    }
}

fn list(req: &Req, st: &mut State) -> Resp {
    st.list_calls += 1;
    if st.rate_limit_next > 0 {
        st.rate_limit_next -= 1;
        return drive_err(403, "userRateLimitExceeded", "User Rate Limit Exceeded");
    }
    let q = req.q("q").unwrap_or("");
    let mut hits: Vec<&FFile> = if q.starts_with("sharedWithMe = true") {
        st.files
            .values()
            .filter(|f| f.shared_with_me && !f.trashed)
            .collect()
    } else if let Some(rest) = q.strip_prefix('\'') {
        let Some((id, _)) = rest.split_once("' in parents") else {
            return drive_err(400, "invalid", "q");
        };
        let id = if id == "root" { ROOT_ID } else { id };
        if req.q("corpora") == Some("drive") && req.q("driveId").is_none() {
            return drive_err(400, "invalid", "driveId");
        }
        st.files
            .values()
            .filter(|f| f.parents.iter().any(|p| p == id) && !f.trashed)
            .collect()
    } else {
        return drive_err(400, "invalid", "unsupported q");
    };
    hits.sort_by(|a, b| a.id.cmp(&b.id));
    let start: usize = req.q("pageToken").and_then(|t| t.parse().ok()).unwrap_or(0);
    let size = req
        .q("pageSize")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100)
        .min(st.page_cap);
    let page: Vec<Value> = hits
        .iter()
        .skip(start)
        .take(size)
        .map(|f| State::json(f))
        .collect();
    let mut v = json!({ "files": page });
    if start + size < hits.len() {
        v["nextPageToken"] = json!((start + size).to_string());
    }
    resp_json(200, v)
}

fn upload_chunk(req: &Req, st: &mut State, sid: &str) -> Resp {
    let range = req
        .headers
        .get("content-range")
        .cloned()
        .unwrap_or_default();
    st.chunk_ranges.push(range.clone());
    let short = st.short_ack_once;
    let Some(up) = st.uploads.get_mut(sid) else {
        return drive_err(404, "notFound", "upload session");
    };
    let spec = range.strip_prefix("bytes ").unwrap_or("");
    let (span, total) = spec.split_once('/').unwrap_or(("", "*"));
    let total: Option<usize> = total.parse().ok();
    let mut body = req.body.clone();
    if span != "*" {
        let start: usize = span
            .split('-')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(usize::MAX);
        if start != up.data.len() {
            return drive_err(400, "badContentRange", "unexpected offset");
        }
        if total.is_none() && !body.len().is_multiple_of(256 * 1024) {
            return drive_err(
                400,
                "badContentRange",
                "chunk size must be a multiple of 256 KiB",
            );
        }
        if short && total.is_none() {
            body.truncate(body.len() / 2);
        }
    }
    up.data.extend_from_slice(&body);
    if short && total.is_none() {
        st.short_ack_once = false;
    }
    let up = st.uploads.get(sid).expect("session");
    if total == Some(up.data.len()) {
        let up = st.uploads.remove(sid).expect("session");
        let id = match up.update {
            Some(id) => {
                let f = st.files.get_mut(&id).expect("file");
                f.data = up.data;
                f.modified = "2025-06-01T12:00:00.000Z".into();
                id
            }
            None => {
                let parent = up.meta["parents"][0].as_str().unwrap_or("root");
                let parent = if parent == "root" { ROOT_ID } else { parent }.to_string();
                let name = up.meta["name"].as_str().unwrap_or("").to_string();
                st.add(&name, "application/octet-stream", &parent, &up.data)
            }
        };
        return resp_json(200, State::json(&st.files[&id]));
    }
    let mut headers = Vec::new();
    if !up.data.is_empty() {
        headers.push((
            "Range".to_string(),
            format!("bytes=0-{}", up.data.len() - 1),
        ));
    }
    Resp {
        status: 308,
        headers,
        body: Vec::new(),
    }
}
