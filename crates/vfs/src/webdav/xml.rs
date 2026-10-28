//! The WebDAV XML documents: PROPFIND requests, `207 Multi-Status`
//! replies and PROPPATCH (modification time).

use chrono::{DateTime, Utc};

use crate::s3::xml::{parse, parse_time, Node};

use super::client::href_path;

/// PROPFIND body asking for exactly the properties Manta shows.
pub const PROPFIND_BODY: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<d:propfind xmlns:d="DAV:"><d:prop>"#,
    "<d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/>",
    "</d:prop></d:propfind>"
);

/// PROPFIND body asking for the quota (RFC 4331).
pub const QUOTA_BODY: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<d:propfind xmlns:d="DAV:"><d:prop>"#,
    "<d:quota-available-bytes/><d:quota-used-bytes/>",
    "</d:prop></d:propfind>"
);

/// One `<response>` of a Multi-Status reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavItem {
    /// Decoded server path of `<href>`, without trailing slash.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub etag: Option<String>,
}

impl DavItem {
    /// Last path segment.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }
}

/// `HTTP/1.1 200 OK` → 200.
fn status_code(s: &str) -> Option<u16> {
    s.split_whitespace().nth(1).and_then(|c| c.parse().ok())
}

/// Parse a `207 Multi-Status` PROPFIND reply. Properties of failed
/// propstats (404 for a property the server doesn't have) are ignored.
pub fn parse_multistatus(xml: &str) -> Result<Vec<DavItem>, String> {
    let root = parse(xml)?;
    if root.name != "multistatus" {
        return Err(format!("unexpected WebDAV reply <{}>", root.name));
    }
    let mut out = Vec::new();
    for r in root.children("response") {
        let Some(href) = r.text_of("href") else {
            continue;
        };
        // A response-level status (no propstat) means the resource failed.
        if r.child("propstat").is_none() {
            continue;
        }
        let mut item = DavItem {
            path: href_path(href),
            is_dir: false,
            size: 0,
            modified: None,
            etag: None,
        };
        for ps in r.children("propstat") {
            let ok = ps
                .text_of("status")
                .and_then(status_code)
                .is_none_or(|c| (200..300).contains(&c));
            if !ok {
                continue;
            }
            let Some(prop) = ps.child("prop") else {
                continue;
            };
            apply_props(prop, &mut item);
        }
        out.push(item);
    }
    Ok(out)
}

fn apply_props(prop: &Node, item: &mut DavItem) {
    if let Some(rt) = prop.child("resourcetype") {
        if rt.child("collection").is_some() {
            item.is_dir = true;
        }
    }
    if let Some(len) = prop.text_of("getcontentlength") {
        item.size = len.trim().parse().unwrap_or(0);
    }
    if let Some(m) = prop.text_of("getlastmodified") {
        item.modified = parse_time(m);
    }
    if let Some(e) = prop.text_of("getetag") {
        let e = e.trim();
        if !e.is_empty() {
            item.etag = Some(e.to_string());
        }
    }
}

/// The quota of a [`QUOTA_BODY`] reply: `(available, used)`. Negative
/// values (Nextcloud: -2 unknown, -3 unlimited) and missing ones are `None`.
pub fn parse_quota(xml: &str) -> Result<(Option<u64>, Option<u64>), String> {
    let root = parse(xml)?;
    let mut avail = None;
    let mut used = None;
    // Depth 0: the folder itself is the only response.
    let Some(r) = root.children("response").next() else {
        return Ok((None, None));
    };
    for ps in r.children("propstat") {
        let ok = ps
            .text_of("status")
            .and_then(status_code)
            .is_none_or(|c| (200..300).contains(&c));
        let Some(prop) = ps.child("prop").filter(|_| ok) else {
            continue;
        };
        let num = |n: &str| {
            prop.text_of(n)
                .and_then(|t| t.trim().parse::<i64>().ok())
                .and_then(|v| u64::try_from(v).ok())
        };
        avail = avail.or(num("quota-available-bytes"));
        used = used.or(num("quota-used-bytes"));
    }
    Ok((avail, used))
}

/// PROPPATCH body setting one property (`name` in the DAV: namespace).
pub fn proppatch_body(name: &str, value: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<d:propertyupdate xmlns:d="DAV:"><d:set><d:prop>"#,
            "<d:{name}>{value}</d:{name}>",
            "</d:prop></d:set></d:propertyupdate>"
        ),
        name = name,
        value = crate::s3::xml::escape(value)
    )
}

/// Did every property of a PROPPATCH reply succeed?
pub fn proppatch_ok(xml: &str) -> bool {
    let Ok(root) = parse(xml) else {
        return false;
    };
    let mut any = false;
    for r in root.children("response") {
        if let Some(s) = r.text_of("status").and_then(status_code) {
            if !(200..300).contains(&s) {
                return false;
            }
            any = true;
        }
        for ps in r.children("propstat") {
            match ps.text_of("status").and_then(status_code) {
                Some(c) if (200..300).contains(&c) => any = true,
                _ => return false,
            }
        }
    }
    any
}

/// An HTTP date (RFC 1123), as `getlastmodified` uses.
pub fn http_date(t: DateTime<Utc>) -> String {
    t.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const APACHE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:ns0="DAV:">
<D:response xmlns:lp1="DAV:" xmlns:lp2="http://apache.org/dav/props/">
<D:href>/dav/docs/</D:href>
<D:propstat><D:prop><lp1:resourcetype><D:collection/></lp1:resourcetype>
<lp1:getlastmodified>Tue, 02 Jan 2024 10:00:00 GMT</lp1:getlastmodified>
<lp1:getetag>"1000-5f0"</lp1:getetag></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
<D:propstat><D:prop><ns0:getcontentlength/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
</D:response>
<D:response><D:href>/dav/docs/a%20b%26c.txt</D:href>
<D:propstat><D:prop><lp1:resourcetype xmlns:lp1="DAV:"/><lp1:getcontentlength xmlns:lp1="DAV:">1234</lp1:getcontentlength>
<lp1:getlastmodified xmlns:lp1="DAV:">Wed, 03 Jan 2024 11:30:00 GMT</lp1:getlastmodified></D:prop>
<D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
<D:response><D:href>https://example.com/dav/docs/sub/</D:href>
<D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>"#;

    #[test]
    fn multistatus() {
        let items = parse_multistatus(APACHE).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].path, "/dav/docs");
        assert!(items[0].is_dir);
        assert_eq!(items[0].etag.as_deref(), Some("\"1000-5f0\""));
        assert_eq!(items[1].path, "/dav/docs/a b&c.txt");
        assert_eq!(items[1].name(), "a b&c.txt");
        assert!(!items[1].is_dir);
        assert_eq!(items[1].size, 1234);
        assert_eq!(
            items[1].modified.unwrap().to_rfc3339(),
            "2024-01-03T11:30:00+00:00"
        );
        assert_eq!(items[2].path, "/dav/docs/sub");
        assert!(items[2].is_dir);
    }

    /// Nextcloud / SabreDAV style: lower-case `d:` prefix, default namespace.
    #[test]
    fn multistatus_nextcloud() {
        let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:oc="http://owncloud.org/ns">
 <d:response><d:href>/remote.php/dav/files/alice/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype><d:getetag>&quot;65a&quot;</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:getcontentlength/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
 </d:response>
 <d:response><d:href>/remote.php/dav/files/alice/Notes.md</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>12</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
</d:multistatus>"#;
        let items = parse_multistatus(x).unwrap();
        assert_eq!(items[0].etag.as_deref(), Some("\"65a\""));
        assert_eq!(items[1].name(), "Notes.md");
        assert_eq!(items[1].size, 12);
        let x2 = r#"<multistatus xmlns="DAV:"><response><href>/x</href><propstat><prop><resourcetype><collection/></resourcetype></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
        assert!(parse_multistatus(x2).unwrap()[0].is_dir);
        assert!(parse_multistatus("<html>no</html>").is_err());
    }

    #[test]
    fn proppatch() {
        let b = proppatch_body("getlastmodified", "Tue, 02 Jan 2024 10:00:00 GMT");
        assert!(b.contains("<d:getlastmodified>Tue, 02 Jan 2024 10:00:00 GMT</d:getlastmodified>"));
        let ok = r#"<D:multistatus xmlns:D="DAV:"><D:response><D:href>/a</D:href><D:propstat><D:prop><D:getlastmodified/></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"#;
        assert!(proppatch_ok(ok));
        let refused = ok.replace("200 OK", "409 Conflict");
        assert!(!proppatch_ok(&refused));
        assert!(!proppatch_ok("garbage"));
        let t = DateTime::parse_from_rfc3339("2024-01-02T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(http_date(t), "Tue, 02 Jan 2024 10:00:00 GMT");
    }

    #[test]
    fn quota() {
        let x = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/f/</d:href>
<d:propstat><d:prop><d:quota-available-bytes>1000</d:quota-available-bytes><d:quota-used-bytes>24</d:quota-used-bytes></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
</d:response></d:multistatus>"#;
        assert_eq!(parse_quota(x).unwrap(), (Some(1000), Some(24)));
        // Nextcloud without a limit: -3.
        let unlimited = x.replace(">1000<", ">-3<");
        assert_eq!(parse_quota(&unlimited).unwrap(), (None, Some(24)));
        let missing = x.replace("200 OK", "404 Not Found");
        assert_eq!(parse_quota(&missing).unwrap(), (None, None));
    }
}
