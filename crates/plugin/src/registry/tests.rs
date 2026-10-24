//! Parsing, language, compatibility, and fetching / downloading from a
//! local HTTP server (never the internet).

use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A tiny HTTP/1.1 server on 127.0.0.1 for tests: `routes` maps a path to
/// a body; anything else is a 404. Stops when dropped.
pub(crate) struct TestServer {
    pub base: String,
    stop: Arc<AtomicBool>,
    pub hits: Arc<AtomicUsize>,
    routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

impl TestServer {
    pub fn start(routes: Vec<(&str, Vec<u8>)>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>> = Arc::new(Mutex::new(
            routes
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        ));
        {
            let (stop, hits, routes) = (stop.clone(), hits.clone(), routes.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let Ok((stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    hits.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.set_nonblocking(false);
                    let mut r = BufReader::new(stream.try_clone().unwrap());
                    let mut first = String::new();
                    let _ = r.read_line(&mut first);
                    loop {
                        let mut l = String::new();
                        if r.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
                            break;
                        }
                    }
                    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = routes.lock().unwrap().get(&path).cloned();
                    let mut s = stream;
                    let resp = match body {
                        Some(b) => {
                            let mut v = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                b.len()
                            )
                            .into_bytes();
                            v.extend_from_slice(&b);
                            v
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec(),
                    };
                    let _ = s.write_all(&resp);
                    let _ = s.flush();
                }
            });
        }
        TestServer {
            base,
            stop,
            hits,
            routes,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn set(&self, path: &str, body: Vec<u8>) {
        self.routes.lock().unwrap().insert(path.to_string(), body);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn entry_json(id: &str, version: &str, url: &str, sha: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": { "en": format!("Plugin {id}"), "de": format!("Erweiterung {id}"), "zh-Hans": format!("插件 {id}") },
        "description": { "en": "Does things." },
        "author": "Anna",
        "repository": format!("https://example.org/{id}"),
        "license": "MIT",
        "latest": { "version": version, "url": url, "sha256": sha },
        "min_manta_version": "0.1.0",
        "platforms": ["any"],
        "runtime": "shell",
        "permissions": ["read_files"],
        "tags": ["demo"]
    })
}

pub(crate) fn registry_json(entries: Vec<serde_json::Value>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "schema": 1, "plugins": entries })).unwrap()
}

const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn parses_and_skips_broken_entries() {
    let mut bad_sha = entry_json("bad-sha", "1", "https://x.org/a.zip", "XYZ");
    bad_sha["latest"]["sha256"] = "XYZ".into();
    let bytes = registry_json(vec![
        entry_json("zeta", "1.0.0", "https://x.org/z.mantaplugin", SHA),
        entry_json("alpha", "2.0.0", "https://x.org/a.mantaplugin", SHA),
        bad_sha,
        entry_json("http-url", "1", "http://x.org/a.zip", SHA),
        entry_json("ftp-url", "1", "ftp://127.0.0.1/a.zip", SHA),
        entry_json("manta.fake", "1", "https://x.org/a.zip", SHA),
        entry_json("alpha", "3", "https://x.org/a.zip", SHA),
        serde_json::json!({"id": "incomplete"}),
    ]);
    let reg = parse(&bytes).unwrap();
    let ids: Vec<&str> = reg.plugins.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "zeta"], "sorted by name");
    assert_eq!(reg.skipped.len(), 6, "{:?}", reg.skipped);
    assert!(parse(b"{\"schema\": 2, \"plugins\": []}")
        .unwrap_err()
        .contains("format 2"));
    assert!(parse(b"nonsense").is_err());
    // The repository's own directory parses.
    let ours = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/registry.json"
    ))
    .unwrap();
    assert!(parse(&ours).unwrap().skipped.is_empty());
}

#[test]
fn texts_follow_the_language() {
    let e: Entry =
        serde_json::from_value(entry_json("demo", "1", "https://x.org/a.zip", SHA)).unwrap();
    use crate::i18n::{with_lang, Lang};
    assert_eq!(with_lang(Lang::De, || e.name()), "Erweiterung demo");
    assert_eq!(with_lang(Lang::Zh, || e.name()), "插件 demo");
    assert_eq!(
        with_lang(Lang::Fr, || e.name()),
        "Plugin demo",
        "falls back to English"
    );
    assert_eq!(with_lang(Lang::De, || e.description()), "Does things.");
}

#[test]
fn compatibility() {
    let mut e: Entry =
        serde_json::from_value(entry_json("demo", "1", "https://x.org/a.zip", SHA)).unwrap();
    assert_eq!(e.incompatibility(), None);
    e.min_manta_version = "999.0".into();
    assert_eq!(
        e.incompatibility(),
        Some(Incompatible::NeedsManta("999.0".into()))
    );
    e.min_manta_version = MANTA_VERSION.into();
    e.platforms = vec!["macos-arm64".into()];
    let arm = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    assert_eq!(e.incompatibility().is_none(), arm);
}

#[test]
fn urls() {
    assert!(check_url("https://raw.githubusercontent.com/x").is_ok());
    assert!(check_url("http://127.0.0.1:8080/r.json").is_ok());
    assert!(check_url("http://localhost/r.json").is_ok());
    assert!(check_url("http://example.org/r.json").is_err());
    assert!(check_url("http://user@127.0.0.1/r.json").is_err());
    assert!(check_url("file:///etc/passwd").is_err());
    assert!(check_url("https://a b").is_err());
}

#[test]
fn default_url_and_override() {
    // Not set in the test environment (would point the app elsewhere).
    if std::env::var_os(URL_ENV).is_none() {
        assert_eq!(registry_url(), DEFAULT_URL);
    }
    assert!(DEFAULT_URL.starts_with("https://raw.githubusercontent.com/web-workers-com/manta/"));
}

#[test]
fn fetch_saves_a_copy_and_falls_back_to_it_offline() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join(CACHE_FILE);
    let body = registry_json(vec![entry_json(
        "demo",
        "1.0.0",
        "https://x.org/a.zip",
        SHA,
    )]);
    let server = TestServer::start(vec![("/registry.json", body)]);
    let f = fetch(&server.url("/registry.json"), Some(&cache)).unwrap();
    assert!(!f.from_cache && f.error.is_none());
    assert_eq!(f.registry.plugins[0].id, "demo");
    assert!(cache.is_file());

    // The server is gone: the saved copy, with the reason.
    drop(server);
    std::thread::sleep(Duration::from_millis(30));
    let offline = fetch("http://127.0.0.1:9/registry.json", Some(&cache)).unwrap();
    assert!(offline.from_cache);
    assert!(offline.error.is_some());
    assert_eq!(offline.registry.plugins[0].id, "demo");
    assert!(offline.fetched_at.is_some());

    // No copy: an error.
    assert!(fetch(
        "http://127.0.0.1:9/registry.json",
        Some(&tmp.path().join("none.json"))
    )
    .is_err());
    // A broken answer is an error too (and falls back).
    let server = TestServer::start(vec![("/r.json", b"<html>".to_vec())]);
    let f = fetch(&server.url("/r.json"), Some(&cache)).unwrap();
    assert!(f.from_cache && f.error.unwrap().contains("damaged"));
    let f = fetch(&server.url("/missing.json"), None).unwrap_err();
    assert!(f.contains("404"), "{f}");
}

#[test]
fn download_verifies_the_checksum() {
    let tmp = tempfile::tempdir().unwrap();
    let data = b"pretend this is a zip".to_vec();
    let mut h = Sha256::new();
    h.update(&data);
    let good = hex(&h.finalize());
    let server = TestServer::start(vec![("/p.mantaplugin", data.clone())]);
    let dest = tmp.path().join("p.mantaplugin");
    download(&server.url("/p.mantaplugin"), &good, &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert_eq!(sha256_file(&dest).unwrap(), good);

    let dest2 = tmp.path().join("q.mantaplugin");
    let e = download(&server.url("/p.mantaplugin"), SHA, &dest2).unwrap_err();
    assert!(e.contains("SHA-256 mismatch"), "{e}");
    assert!(
        !dest2.exists() && !dest2.with_extension("part").exists(),
        "nothing kept"
    );
    assert!(download("http://example.org/p.zip", &good, &dest2)
        .unwrap_err()
        .contains("https://"));
}
