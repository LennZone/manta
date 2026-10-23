//! Parsing and building the few S3 XML documents we need.

use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;

/// A minimal XML element tree (namespaces stripped, attributes ignored).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_string(),
        None => s.into_owned(),
    }
}

/// Parse a document into its root element.
pub fn parse(xml: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| format!("invalid XML: {e}"))?;
        match ev {
            Event::Start(e) => stack.push(Node {
                name: local_name(e.name().as_ref()),
                ..Default::default()
            }),
            Event::Empty(e) => {
                let n = Node {
                    name: local_name(e.name().as_ref()),
                    ..Default::default()
                };
                if let Some(top) = stack.last_mut() {
                    top.children.push(n);
                }
            }
            Event::End(_) => {
                let n = stack.pop().ok_or("unbalanced XML")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(n),
                    None => return Err("unbalanced XML".into()),
                }
            }
            Event::Text(t) => {
                let s = t.decode().map_err(|e| format!("invalid XML text: {e}"))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&s);
                }
            }
            Event::CData(t) => {
                let s = t.decode().map_err(|e| format!("invalid XML text: {e}"))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&s);
                }
            }
            Event::GeneralRef(r) => {
                let ch = match r
                    .resolve_char_ref()
                    .map_err(|e| format!("invalid XML: {e}"))?
                {
                    Some(c) => c.to_string(),
                    None => match r
                        .decode()
                        .map_err(|e| format!("invalid XML: {e}"))?
                        .as_ref()
                    {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        other => format!("&{other};"),
                    },
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&ch);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let doc = stack.pop().ok_or("empty XML")?;
    if !stack.is_empty() {
        return Err("truncated XML".into());
    }
    doc.children
        .into_iter()
        .next()
        .ok_or_else(|| "empty XML document".into())
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .or_else(|| DateTime::parse_from_rfc2822(s.trim()).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// `<Error>` body of a failed request.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct S3ErrorBody {
    pub code: String,
    pub message: String,
    /// Set by region errors (`AuthorizationHeaderMalformed`, ...).
    pub region: Option<String>,
}

pub fn parse_error(xml: &str) -> Option<S3ErrorBody> {
    let root = parse(xml).ok()?;
    if root.name != "Error" {
        return None;
    }
    Some(S3ErrorBody {
        code: root.text_of("Code").unwrap_or_default().to_string(),
        message: root.text_of("Message").unwrap_or_default().to_string(),
        region: root
            .text_of("Region")
            .filter(|r| !r.is_empty())
            .map(str::to_string),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub name: String,
    pub created: Option<DateTime<Utc>>,
}

pub fn parse_list_buckets(xml: &str) -> Result<Vec<Bucket>, String> {
    let root = parse(xml)?;
    let Some(buckets) = root.child("Buckets") else {
        return Ok(Vec::new());
    };
    Ok(buckets
        .children("Bucket")
        .filter_map(|b| {
            Some(Bucket {
                name: b.text_of("Name")?.to_string(),
                created: b.text_of("CreationDate").and_then(parse_time),
            })
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub key: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

/// One page of a ListObjectsV2 response.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListPage {
    pub prefixes: Vec<String>,
    pub objects: Vec<Object>,
    pub next_token: Option<String>,
}

pub fn parse_list_objects(xml: &str) -> Result<ListPage, String> {
    let root = parse(xml)?;
    if root.name != "ListBucketResult" {
        return Err(format!("unexpected response <{}>", root.name));
    }
    let truncated = root
        .text_of("IsTruncated")
        .is_some_and(|t| t.trim() == "true");
    Ok(ListPage {
        prefixes: root
            .children("CommonPrefixes")
            .filter_map(|p| p.text_of("Prefix").map(str::to_string))
            .collect(),
        objects: root
            .children("Contents")
            .filter_map(|c| {
                Some(Object {
                    key: c.text_of("Key")?.to_string(),
                    size: c
                        .text_of("Size")
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(0),
                    modified: c.text_of("LastModified").and_then(parse_time),
                })
            })
            .collect(),
        next_token: if truncated {
            root.text_of("NextContinuationToken").map(str::to_string)
        } else {
            None
        },
    })
}

pub fn parse_upload_id(xml: &str) -> Result<String, String> {
    let root = parse(xml)?;
    root.text_of("UploadId")
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "no UploadId in response".into())
}

/// `(key, code, message)` of every key a DeleteObjects call failed on.
pub fn parse_delete_errors(xml: &str) -> Result<Vec<(String, String, String)>, String> {
    let root = parse(xml)?;
    Ok(root
        .children("Error")
        .map(|e| {
            (
                e.text_of("Key").unwrap_or_default().to_string(),
                e.text_of("Code").unwrap_or_default().to_string(),
                e.text_of("Message").unwrap_or_default().to_string(),
            )
        })
        .collect())
}

/// One uploaded part of a multipart upload (ListParts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub number: u32,
    pub etag: String,
    pub size: u64,
}

/// Parts of a ListParts page, and the marker for the next page.
pub fn parse_list_parts(xml: &str) -> Result<(Vec<Part>, Option<String>), String> {
    let root = parse(xml)?;
    let parts = root
        .children("Part")
        .filter_map(|p| {
            Some(Part {
                number: p.text_of("PartNumber")?.trim().parse().ok()?,
                etag: p.text_of("ETag").unwrap_or_default().to_string(),
                size: p
                    .text_of("Size")
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0),
            })
        })
        .collect();
    let truncated = root
        .text_of("IsTruncated")
        .is_some_and(|t| t.trim() == "true");
    let next = root
        .text_of("NextPartNumberMarker")
        .filter(|m| truncated && !m.is_empty())
        .map(str::to_string);
    Ok((parts, next))
}

pub fn complete_multipart_body(parts: &[(u32, String)]) -> String {
    let mut s = String::from("<CompleteMultipartUpload>");
    for (n, etag) in parts {
        s.push_str(&format!(
            "<Part><PartNumber>{n}</PartNumber><ETag>{}</ETag></Part>",
            escape(etag)
        ));
    }
    s.push_str("</CompleteMultipartUpload>");
    s
}

pub fn delete_objects_body(keys: &[String]) -> String {
    let mut s = String::from("<Delete><Quiet>true</Quiet>");
    for k in keys {
        s.push_str(&format!("<Object><Key>{}</Key></Object>", escape(k)));
    }
    s.push_str("</Delete>");
    s
}

pub fn create_bucket_body(region: &str) -> String {
    format!(
        "<CreateBucketConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
         <LocationConstraint>{}</LocationConstraint></CreateBucketConfiguration>",
        escape(region)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_objects_v2() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>photos</Name><Prefix>2024/</Prefix><KeyCount>3</KeyCount><MaxKeys>1000</MaxKeys>
  <Delimiter>/</Delimiter><IsTruncated>true</IsTruncated>
  <NextContinuationToken>1ueGcxLPRx1Tr/XYExHnhbYLgveDs2J/wm36Hy4vbOwM=</NextContinuationToken>
  <Contents>
    <Key>2024/</Key><LastModified>2024-05-01T10:00:00.000Z</LastModified>
    <ETag>"d41d8cd98f00b204e9800998ecf8427e"</ETag><Size>0</Size><StorageClass>STANDARD</StorageClass>
  </Contents>
  <Contents>
    <Key>2024/ Tom &amp; Jerry &lt;1&gt;.jpg</Key><LastModified>2024-05-02T11:30:15.000Z</LastModified>
    <Size>1048576</Size>
  </Contents>
  <CommonPrefixes><Prefix>2024/raw/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>2024/édits/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let page = parse_list_objects(xml).unwrap();
        assert_eq!(page.prefixes, vec!["2024/raw/", "2024/édits/"]);
        assert_eq!(page.objects.len(), 2);
        assert_eq!(page.objects[1].key, "2024/ Tom & Jerry <1>.jpg");
        assert_eq!(page.objects[1].size, 1_048_576);
        assert_eq!(
            page.objects[1].modified.unwrap().to_rfc3339(),
            "2024-05-02T11:30:15+00:00"
        );
        assert_eq!(
            page.next_token.as_deref(),
            Some("1ueGcxLPRx1Tr/XYExHnhbYLgveDs2J/wm36Hy4vbOwM=")
        );
    }

    #[test]
    fn list_not_truncated_has_no_token() {
        let xml = "<ListBucketResult><IsTruncated>false</IsTruncated>\
                   <NextContinuationToken>x</NextContinuationToken></ListBucketResult>";
        assert_eq!(parse_list_objects(xml).unwrap().next_token, None);
        assert!(parse_list_objects("<Error><Code>X</Code></Error>").is_err());
    }

    #[test]
    fn list_buckets() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>abc</ID><DisplayName>me</DisplayName></Owner>
  <Buckets>
    <Bucket><Name>alpha</Name><CreationDate>2023-01-02T03:04:05.000Z</CreationDate></Bucket>
    <Bucket><Name>beta</Name><CreationDate>2023-02-02T03:04:05Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;
        let b = parse_list_buckets(xml).unwrap();
        assert_eq!(
            b.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        assert!(b[0].created.is_some());
        assert!(
            parse_list_buckets("<ListAllMyBucketsResult><Buckets/></ListAllMyBucketsResult>")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn errors() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Error><Code>AuthorizationHeaderMalformed</Code><Message>the region 'us-east-1' is wrong; expecting 'eu-west-1'</Message><Region>eu-west-1</Region><RequestId>X</RequestId></Error>"#;
        let e = parse_error(xml).unwrap();
        assert_eq!(e.code, "AuthorizationHeaderMalformed");
        assert_eq!(e.region.as_deref(), Some("eu-west-1"));
        assert!(e.message.contains("eu-west-1"));
        assert_eq!(parse_error("<ListBucketResult/>"), None);
        assert_eq!(parse_error("not xml <"), None);
    }

    #[test]
    fn multipart_and_delete() {
        assert_eq!(
            parse_upload_id(
                "<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key>\
                             <UploadId>VXBsb2FkIElE</UploadId></InitiateMultipartUploadResult>"
            )
            .unwrap(),
            "VXBsb2FkIElE"
        );
        let body = complete_multipart_body(&[(1, "\"a\"".into()), (2, "\"b\"".into())]);
        assert!(body.contains("<Part><PartNumber>2</PartNumber><ETag>&quot;b&quot;</ETag></Part>"));
        let del = delete_objects_body(&["a&b".into(), "c".into()]);
        assert_eq!(
            del,
            "<Delete><Quiet>true</Quiet><Object><Key>a&amp;b</Key></Object>\
             <Object><Key>c</Key></Object></Delete>"
        );
        let errs = parse_delete_errors(
            "<DeleteResult><Error><Key>x</Key><Code>AccessDenied</Code><Message>no</Message></Error></DeleteResult>",
        )
        .unwrap();
        assert_eq!(errs, vec![("x".into(), "AccessDenied".into(), "no".into())]);
        // Round trip through the parser.
        let root = parse(&del).unwrap();
        assert_eq!(
            root.children("Object").next().unwrap().text_of("Key"),
            Some("a&b")
        );
    }

    #[test]
    fn list_parts() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListPartsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Bucket>b</Bucket><Key>k</Key><UploadId>u</UploadId>
  <IsTruncated>true</IsTruncated><NextPartNumberMarker>2</NextPartNumberMarker>
  <Part><PartNumber>1</PartNumber><ETag>"e1"</ETag><Size>16777216</Size></Part>
  <Part><PartNumber>2</PartNumber><ETag>&quot;e2&quot;</ETag><Size>16777216</Size></Part>
</ListPartsResult>"#;
        let (parts, next) = parse_list_parts(xml).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(
            parts[0],
            Part {
                number: 1,
                etag: "\"e1\"".into(),
                size: 16_777_216
            }
        );
        assert_eq!(parts[1].etag, "\"e2\"");
        assert_eq!(next.as_deref(), Some("2"));
        let (_, next) =
            parse_list_parts("<ListPartsResult><IsTruncated>false</IsTruncated></ListPartsResult>")
                .unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn http_dates() {
        assert!(parse_time("Mon, 12 Oct 2009 17:50:00 GMT").is_some());
        assert!(parse_time("2009-10-12T17:50:30.000Z").is_some());
        assert!(parse_time("garbage").is_none());
    }
}
