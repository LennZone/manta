//! A small in-memory WebDAV server on 127.0.0.1 for the unit tests:
//! PROPFIND (depth 0/1), GET (with Range), PUT (Content-Length or chunked),
//! MKCOL, DELETE, MOVE, COPY, PROPPATCH (getlastmodified), Basic or Digest
//! authentication, an optional root prefix and failure knobs.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};

use super::client::{decode, encode_path, hex, parse_challenges};

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub dir: bool,
    pub data: Vec<u8>,
    pub modified: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthMode {
    None,
    Basic,
    Digest,
}

#[derive(Debug)]
pub(crate) struct State {
    /// Decoded server paths ("" is the prefix folder itself).
    pub nodes: BTreeMap<String, Node>,
    pub auth: AuthMode,
    pub user: String,
    pub password: String,
    pub nonce: String,
    /// Accept each nonce this many times, then answer `stale=true`.
    pub nonce_uses: usize,
    used: usize,
    nonce_gen: u64,
    /// Answer the next N requests with 503.
    pub fail_503_next: usize,
    pub ranges: bool,
    pub allow_mtime: bool,
    /// (method, decoded path, chunked) of every request.
    pub log: Vec<(String, String, bool)>,
    pub unauthorized: usize,
}

impl State {
    pub fn add_file(&mut self, path: &str, data: &[u8]) {
        self.nodes.insert(
            path.trim_end_matches('/').to_string(),
            Node {
                dir: false,
                data: data.to_vec(),
                modified: Utc::now(),
            },
        );
    }

    pub fn add_dir(&mut self, path: &str) {
        self.nodes.insert(
            path.trim_end_matches('/').to_string(),
            Node {
                dir: true,
                data: Vec::new(),
                modified: Utc::now(),
            },
        );
    }

    pub fn file(&self, path: &str) -> Option<&Vec<u8>> {
        self.nodes.get(path).filter(|n| !n.dir).map(|n| &n.data)
    }

    pub fn count(&self, method: &str) -> usize {
        self.log.iter().filter(|(m, _, _)| m == method).count()
    }
}

pub(crate) struct FakeDav {
    pub port: u16,
    pub prefix: String,
    pub state: Arc<Mutex<State>>,
}

impl FakeDav {
    /// A server whose WebDAV tree starts at `prefix` (e.g. "/dav" or "").
    pub fn start(prefix: &str, auth: AuthMode) -> FakeDav {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let prefix = prefix.trim_end_matches('/').to_string();
        let mut nodes = BTreeMap::new();
        // The prefix and its parents exist as folders.
        let mut p = String::new();
        nodes.insert(
            String::new(),
            Node {
                dir: true,
                data: Vec::new(),
                modified: Utc::now(),
            },
        );
        for c in prefix.split('/').filter(|c| !c.is_empty()) {
            p.push('/');
            p.push_str(c);
            nodes.insert(
                p.clone(),
                Node {
                    dir: true,
                    data: Vec::new(),
                    modified: Utc::now(),
                },
            );
        }
        let state = Arc::new(Mutex::new(State {
            nodes,
            auth,
            user: "alice".into(),
            password: "s3cret pass".into(),
            nonce: "nonce-0".into(),
            nonce_uses: usize::MAX,
            used: 0,
            nonce_gen: 0,
            fail_503_next: 0,
            ranges: true,
            allow_mtime: true,
            log: Vec::new(),
            unauthorized: 0,
        }));
        let st = state.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let st = st.clone();
                thread::spawn(move || handle(stream, &st));
            }
        });
        FakeDav {
            port,
            prefix,
            state,
        }
    }

    pub fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    /// Server path of a Manta path.
    pub fn path(&self, p: &str) -> String {
        format!("{}{}", self.prefix, p.trim_end_matches('/'))
    }
}

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    chunked: bool,
    target: String,
}

impl Req {
    fn h(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }
}

fn read_req(r: &mut BufReader<TcpStream>) -> Option<Req> {
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut first = line.split_whitespace();
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut l = String::new();
        r.read_line(&mut l).ok()?;
        let l = l.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let get = |k: &str| {
        headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v): &(String, String)| v.clone())
    };
    let chunked = get("transfer-encoding").is_some_and(|v| v.contains("chunked"));
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            r.read_line(&mut size).ok()?;
            let n = usize::from_str_radix(size.trim().split(';').next()?, 16).ok()?;
            if n == 0 {
                let mut end = String::new();
                r.read_line(&mut end).ok()?;
                break;
            }
            let mut chunk = vec![0; n];
            r.read_exact(&mut chunk).ok()?;
            body.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            r.read_exact(&mut crlf).ok()?;
        }
    } else if let Some(len) = get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body = vec![0; len];
        r.read_exact(&mut body).ok()?;
    }
    let raw_path = target.split('?').next().unwrap_or("").to_string();
    Some(Req {
        method,
        path: decode(&raw_path).trim_end_matches('/').to_string(),
        headers,
        body,
        chunked,
        target,
    })
}

struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn resp(status: u16) -> Resp {
    Resp {
        status,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn handle(stream: TcpStream, state: &Arc<Mutex<State>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut out = write_half;
    // Keep-alive: serve requests until the client closes.
    while let Some(req) = read_req(&mut reader) {
        let r = route(&req, state);
        let mut head = format!(
            "HTTP/1.1 {} X\r\nContent-Length: {}\r\n",
            r.status,
            r.body.len()
        );
        for (k, v) in &r.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        if out.write_all(head.as_bytes()).is_err() {
            return;
        }
        if req.method != "HEAD" && out.write_all(&r.body).is_err() {
            return;
        }
        let _ = out.flush();
    }
}

fn md5_hex(s: &str) -> String {
    use md5::Digest as _;
    hex(&md5::Md5::digest(s.as_bytes()))
}

fn check_auth(req: &Req, st: &mut State) -> Option<Resp> {
    let challenge = |st: &State, stale: bool| {
        let mut r = resp(401);
        match st.auth {
            AuthMode::Basic => r
                .headers
                .push(("WWW-Authenticate".into(), "Basic realm=\"fake\"".into())),
            _ => r.headers.push((
                "WWW-Authenticate".into(),
                format!(
                    "Digest realm=\"fake\", qop=\"auth\", nonce=\"{}\", opaque=\"op\", algorithm=MD5{}",
                    st.nonce,
                    if stale { ", stale=true" } else { "" }
                ),
            )),
        }
        r
    };
    let auth = req.h("authorization").unwrap_or("");
    match st.auth {
        AuthMode::None => None,
        AuthMode::Basic => {
            use base64::Engine as _;
            let want = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", st.user, st.password));
            if auth == format!("Basic {want}") {
                None
            } else {
                st.unauthorized += 1;
                Some(challenge(st, false))
            }
        }
        AuthMode::Digest => {
            let c = parse_challenges(&[auth]);
            let Some(c) = c.first().filter(|c| c.scheme == "digest") else {
                st.unauthorized += 1;
                return Some(challenge(st, false));
            };
            let p = |k: &str| {
                c.params
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let ha1 = md5_hex(&format!("{}:fake:{}", st.user, st.password));
            let ha2 = md5_hex(&format!("{}:{}", req.method, p("uri")));
            let want = md5_hex(&format!(
                "{ha1}:{}:{}:{}:auth:{ha2}",
                p("nonce"),
                p("nc"),
                p("cnonce")
            ));
            let uri_ok = p("uri") == req.target;
            if p("username") != st.user || p("response") != want || !uri_ok {
                st.unauthorized += 1;
                return Some(challenge(st, false));
            }
            if p("nonce") != st.nonce {
                st.unauthorized += 1;
                return Some(challenge(st, true));
            }
            st.used += 1;
            if st.used > st.nonce_uses {
                st.used = 0;
                st.nonce_gen += 1;
                st.nonce = format!("nonce-{}", st.nonce_gen);
                st.unauthorized += 1;
                return Some(challenge(st, true));
            }
            None
        }
    }
}

fn parent(p: &str) -> String {
    p.rfind('/').map_or(String::new(), |i| p[..i].to_string())
}

fn http_date(t: DateTime<Utc>) -> String {
    t.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn prop_xml(path: &str, n: &Node) -> String {
    let href = format!("{}{}", encode_path(path), if n.dir { "/" } else { "" });
    let href = if href.is_empty() {
        "/".to_string()
    } else {
        href
    };
    let rt = if n.dir {
        "<D:resourcetype><D:collection/></D:resourcetype>".to_string()
    } else {
        format!(
            "<D:resourcetype/><D:getcontentlength>{}</D:getcontentlength>",
            n.data.len()
        )
    };
    format!(
        "<D:response><D:href>{href}</D:href><D:propstat><D:prop>{rt}<D:getlastmodified>{}</D:getlastmodified><D:getetag>\"{}-{}\"</D:getetag></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
        http_date(n.modified),
        n.data.len(),
        n.modified.timestamp()
    )
}

fn dest_path(req: &Req) -> Option<String> {
    let d = req.h("destination")?;
    let path = match d.find("://") {
        Some(i) => {
            let rest = &d[i + 3..];
            &rest[rest.find('/')?..]
        }
        None => d,
    };
    Some(decode(path).trim_end_matches('/').to_string())
}

fn route(req: &Req, state: &Arc<Mutex<State>>) -> Resp {
    let mut st = state.lock().unwrap();
    st.log
        .push((req.method.clone(), req.path.clone(), req.chunked));
    if st.fail_503_next > 0 {
        st.fail_503_next -= 1;
        return resp(503);
    }
    if let Some(r) = check_auth(req, &mut st) {
        return r;
    }
    let path = req.path.clone();
    match req.method.as_str() {
        "PROPFIND" => {
            let Some(n) = st.nodes.get(&path).cloned() else {
                return resp(404);
            };
            let mut xml = String::from(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><D:multistatus xmlns:D=\"DAV:\">",
            );
            xml.push_str(&prop_xml(&path, &n));
            if n.dir && req.h("depth") != Some("0") {
                let prefix = format!("{path}/");
                for (p, c) in st.nodes.iter() {
                    if let Some(rest) = p.strip_prefix(&prefix) {
                        if !rest.is_empty() && !rest.contains('/') {
                            xml.push_str(&prop_xml(p, c));
                        }
                    }
                }
            }
            xml.push_str("</D:multistatus>");
            Resp {
                status: 207,
                headers: vec![(
                    "Content-Type".into(),
                    "application/xml; charset=utf-8".into(),
                )],
                body: xml.into_bytes(),
            }
        }
        "GET" | "HEAD" => {
            let Some(n) = st.nodes.get(&path).filter(|n| !n.dir).cloned() else {
                return resp(404);
            };
            if let Some(start) = req
                .h("range")
                .filter(|_| st.ranges)
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.strip_suffix('-'))
                .and_then(|r| r.parse::<usize>().ok())
            {
                if start >= n.data.len() {
                    return resp(416);
                }
                return Resp {
                    status: 206,
                    headers: vec![(
                        "Content-Range".into(),
                        format!("bytes {start}-{}/{}", n.data.len() - 1, n.data.len()),
                    )],
                    body: n.data[start..].to_vec(),
                };
            }
            Resp {
                status: 200,
                headers: vec![("ETag".into(), format!("\"{}\"", n.data.len()))],
                body: n.data,
            }
        }
        "PUT" => {
            if !st.nodes.get(&parent(&path)).is_some_and(|n| n.dir) {
                return resp(409);
            }
            if st.nodes.get(&path).is_some_and(|n| n.dir) {
                return resp(405);
            }
            let existed = st.nodes.contains_key(&path);
            st.add_file(&path, &req.body);
            resp(if existed { 204 } else { 201 })
        }
        "MKCOL" => {
            if st.nodes.contains_key(&path) {
                return resp(405);
            }
            if !st.nodes.get(&parent(&path)).is_some_and(|n| n.dir) {
                return resp(409);
            }
            st.add_dir(&path);
            resp(201)
        }
        "DELETE" => {
            if !st.nodes.contains_key(&path) || path.is_empty() {
                return resp(404);
            }
            let prefix = format!("{path}/");
            st.nodes
                .retain(|p, _| *p != path && !p.starts_with(&prefix));
            resp(204)
        }
        "MOVE" | "COPY" => {
            let Some(dst) = dest_path(req) else {
                return resp(400);
            };
            if !st.nodes.contains_key(&path) {
                return resp(404);
            }
            if !st.nodes.get(&parent(&dst)).is_some_and(|n| n.dir) {
                return resp(409);
            }
            let exists = st.nodes.contains_key(&dst);
            if exists && req.h("overwrite") == Some("F") {
                return resp(412);
            }
            let dprefix = format!("{dst}/");
            st.nodes
                .retain(|p, _| *p != dst && !p.starts_with(&dprefix));
            let prefix = format!("{path}/");
            let moved: Vec<(String, Node)> = st
                .nodes
                .iter()
                .filter(|(p, _)| **p == path || p.starts_with(&prefix))
                .map(|(p, n)| (format!("{dst}{}", &p[path.len()..]), n.clone()))
                .collect();
            if req.method == "MOVE" {
                st.nodes
                    .retain(|p, _| *p != path && !p.starts_with(&prefix));
            }
            for (p, n) in moved {
                st.nodes.insert(p, n);
            }
            resp(if exists { 204 } else { 201 })
        }
        "PROPPATCH" => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            if !st.nodes.contains_key(&path) {
                return resp(404);
            }
            let ok = st.allow_mtime && body.contains("getlastmodified>");
            if ok {
                let v = body
                    .split("getlastmodified>")
                    .nth(1)
                    .and_then(|s| s.split('<').next())
                    .unwrap_or("");
                if let Ok(t) = DateTime::parse_from_rfc2822(v) {
                    if let Some(n) = st.nodes.get_mut(&path) {
                        n.modified = t.with_timezone(&Utc);
                    }
                }
            }
            let status = if ok { "200 OK" } else { "403 Forbidden" };
            Resp {
                status: 207,
                headers: Vec::new(),
                body: format!(
                    "<?xml version=\"1.0\"?><D:multistatus xmlns:D=\"DAV:\"><D:response><D:href>{}</D:href><D:propstat><D:prop><D:getlastmodified/></D:prop><D:status>HTTP/1.1 {status}</D:status></D:propstat></D:response></D:multistatus>",
                    encode_path(&path)
                )
                .into_bytes(),
            }
        }
        _ => resp(405),
    }
}
