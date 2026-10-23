//! AWS Signature Version 4 (header-based) for S3 requests.
//!
//! Only what S3 needs: canonical request, string to sign, signing key and
//! the `Authorization` header. Verified against AWS's published test
//! vectors (see the tests at the bottom).

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// SHA-256 of the empty string, the payload hash of body-less requests.
#[cfg(test)]
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Access key pair (plus a session token for temporary credentials).
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print secrets into logs.
        f.debug_struct("Credentials")
            .field("access_key", &self.access_key)
            .finish_non_exhaustive()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 percent-encoding as SigV4 wants it: everything except
/// `A-Z a-z 0-9 - _ . ~` is encoded; `/` is kept when `keep_slash`.
pub fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The query string in canonical form (also usable as the real query).
pub fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k, false), uri_encode(v, false)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// What gets signed.
pub struct SignInput<'a> {
    pub method: &'a str,
    /// Already URI-encoded path, e.g. `/bucket/my%20file.txt`.
    pub canonical_uri: &'a str,
    /// Raw (unencoded) query parameters.
    pub query: &'a [(String, String)],
    /// All headers to sign; must include `host` and `x-amz-date`.
    pub headers: &'a [(String, String)],
    /// Hex SHA-256 of the body, or `UNSIGNED-PAYLOAD`.
    pub payload_hash: &'a str,
}

/// Canonical request and the `SignedHeaders` list.
pub fn canonical_request(inp: &SignInput<'_>) -> (String, String) {
    let mut hdrs: Vec<(String, String)> = Vec::new();
    for (k, v) in inp.headers {
        let name = k.trim().to_ascii_lowercase();
        let value = v.split_whitespace().collect::<Vec<_>>().join(" ");
        match hdrs.iter_mut().find(|(n, _)| *n == name) {
            Some((_, existing)) => {
                existing.push(',');
                existing.push_str(&value);
            }
            None => hdrs.push((name, value)),
        }
    }
    hdrs.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = hdrs.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = hdrs
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let uri = if inp.canonical_uri.is_empty() {
        "/"
    } else {
        inp.canonical_uri
    };
    let req = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        inp.method,
        uri,
        canonical_query(inp.query),
        canonical_headers,
        signed,
        inp.payload_hash
    );
    (req, signed)
}

pub fn string_to_sign(amz_date: &str, scope: &str, canonical: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    )
}

pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// The `Authorization` header value. `amz_date` is `YYYYMMDDTHHMMSSZ` and
/// must equal the signed `x-amz-date` header.
pub fn authorization(
    creds: &Credentials,
    region: &str,
    service: &str,
    amz_date: &str,
    inp: &SignInput<'_>,
) -> String {
    let date = &amz_date[..8.min(amz_date.len())];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let (canonical, signed) = canonical_request(inp);
    let sts = string_to_sign(amz_date, &scope, &canonical);
    let key = signing_key(&creds.secret_key, date, region, service);
    let signature = hex(&hmac(&key, sts.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",
        creds.access_key
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn suite_creds() -> Credentials {
        Credentials {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        }
    }

    fn s3_creds() -> Credentials {
        Credentials {
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        }
    }

    /// aws-sig-v4-test-suite: get-vanilla
    #[test]
    fn get_vanilla() {
        let headers = h(&[
            ("Host", "example.amazonaws.com"),
            ("X-Amz-Date", "20150830T123600Z"),
        ]);
        let inp = SignInput {
            method: "GET",
            canonical_uri: "/",
            query: &[],
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        let (creq, _) = canonical_request(&inp);
        assert_eq!(
            creq,
            "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            authorization(
                &suite_creds(),
                "us-east-1",
                "service",
                "20150830T123600Z",
                &inp
            ),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    /// aws-sig-v4-test-suite: get-vanilla-query-order-key-case
    #[test]
    fn get_vanilla_query_order_key_case() {
        let headers = h(&[
            ("Host", "example.amazonaws.com"),
            ("X-Amz-Date", "20150830T123600Z"),
        ]);
        let query = h(&[("Param2", "value2"), ("Param1", "value1")]);
        let inp = SignInput {
            method: "GET",
            canonical_uri: "/",
            query: &query,
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        assert!(canonical_request(&inp)
            .0
            .contains("\nParam1=value1&Param2=value2\n"));
        let auth = authorization(
            &suite_creds(),
            "us-east-1",
            "service",
            "20150830T123600Z",
            &inp,
        );
        assert!(
            auth.ends_with(
                "Signature=b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"
            ),
            "{auth}"
        );
    }

    /// aws-sig-v4-test-suite: post-vanilla
    #[test]
    fn post_vanilla() {
        let headers = h(&[
            ("Host", "example.amazonaws.com"),
            ("X-Amz-Date", "20150830T123600Z"),
        ]);
        let inp = SignInput {
            method: "POST",
            canonical_uri: "/",
            query: &[],
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        let auth = authorization(
            &suite_creds(),
            "us-east-1",
            "service",
            "20150830T123600Z",
            &inp,
        );
        assert!(
            auth.ends_with(
                "Signature=5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b"
            ),
            "{auth}"
        );
    }

    /// S3 docs example: GET Object with a Range header.
    #[test]
    fn s3_get_object_example() {
        let headers = h(&[
            ("Host", "examplebucket.s3.amazonaws.com"),
            ("Range", "bytes=0-9"),
            ("x-amz-content-sha256", EMPTY_SHA256),
            ("x-amz-date", "20130524T000000Z"),
        ]);
        let inp = SignInput {
            method: "GET",
            canonical_uri: "/test.txt",
            query: &[],
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        assert_eq!(
            authorization(&s3_creds(), "us-east-1", "s3", "20130524T000000Z", &inp),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    /// S3 docs example: PUT Object with a body, `$` in the key.
    #[test]
    fn s3_put_object_example() {
        let body = b"Welcome to Amazon S3.";
        let hash = sha256_hex(body);
        assert_eq!(
            hash,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let headers = h(&[
            ("Host", "examplebucket.s3.amazonaws.com"),
            ("Date", "Fri, 24 May 2013 00:00:00 GMT"),
            ("x-amz-date", "20130524T000000Z"),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ("x-amz-content-sha256", &hash),
        ]);
        let uri = format!("/{}", uri_encode("test$file.text", true));
        assert_eq!(uri, "/test%24file.text");
        let inp = SignInput {
            method: "PUT",
            canonical_uri: &uri,
            query: &[],
            headers: &headers,
            payload_hash: &hash,
        };
        let auth = authorization(&s3_creds(), "us-east-1", "s3", "20130524T000000Z", &inp);
        assert!(
            auth.ends_with(
                "Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
            ),
            "{auth}"
        );
    }

    /// S3 docs example: GET Bucket (list objects) with query parameters.
    #[test]
    fn s3_list_objects_example() {
        let headers = h(&[
            ("Host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-date", "20130524T000000Z"),
            ("x-amz-content-sha256", EMPTY_SHA256),
        ]);
        let query = h(&[("max-keys", "2"), ("prefix", "J")]);
        let inp = SignInput {
            method: "GET",
            canonical_uri: "/",
            query: &query,
            headers: &headers,
            payload_hash: EMPTY_SHA256,
        };
        let auth = authorization(&s3_creds(), "us-east-1", "s3", "20130524T000000Z", &inp);
        assert!(
            auth.ends_with(
                "Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
            ),
            "{auth}"
        );
    }

    #[test]
    fn encoding_rules() {
        assert_eq!(uri_encode("a b/c~d+e", true), "a%20b/c~d%2Be");
        assert_eq!(uri_encode("a/b", false), "a%2Fb");
        assert_eq!(uri_encode("ü", true), "%C3%BC");
        assert_eq!(
            canonical_query(&h(&[("uploads", ""), ("a b", "x/y")])),
            "a%20b=x%2Fy&uploads="
        );
    }
}
