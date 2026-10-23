//! AWS CLI profiles: `~/.aws/credentials` and `~/.aws/config`.
//!
//! Supports static keys (`aws_access_key_id`, `aws_secret_access_key`,
//! optional `aws_session_token`) in either file, plus `region` and
//! `endpoint_url` from the config file. SSO, `credential_process` and
//! assume-role profiles are not supported (a clear error says so).

use std::collections::BTreeMap;
use std::path::PathBuf;

use manta_core::{VfsError, VfsResult};

use super::sigv4::Credentials;

type Ini = BTreeMap<String, BTreeMap<String, String>>;

/// Parse an AWS-style INI file. Section names are returned verbatim
/// (`default`, `profile work`, ...); keys are lowercased.
pub fn parse_ini(text: &str) -> Ini {
    let mut out = Ini::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
            out.entry(name.clone()).or_default();
            section = Some(name);
            continue;
        }
        if let (Some(sec), Some((k, v))) = (&section, line.split_once('=')) {
            out.entry(sec.clone())
                .or_default()
                .insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    out
}

fn credentials_file() -> Option<PathBuf> {
    std::env::var_os("AWS_SHARED_CREDENTIALS_FILE")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".aws/credentials")))
}

fn config_file() -> Option<PathBuf> {
    std::env::var_os("AWS_CONFIG_FILE")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".aws/config")))
}

fn read(path: Option<PathBuf>) -> Ini {
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_ini(&t))
        .unwrap_or_default()
}

/// The profile to use for `requested` (`""` = `$AWS_PROFILE` or `default`).
pub fn effective_profile(requested: &str) -> String {
    let r = requested.trim();
    if !r.is_empty() {
        return r.to_string();
    }
    std::env::var("AWS_PROFILE")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "default".into())
}

/// Names of all profiles in both files, sorted, `default` first.
pub fn list_profiles() -> Vec<String> {
    profile_names(&read(credentials_file()), &read(config_file()))
}

pub fn profile_names(creds: &Ini, config: &Ini) -> Vec<String> {
    let mut names: Vec<String> = creds.keys().cloned().collect();
    for k in config.keys() {
        let n = k.strip_prefix("profile ").unwrap_or(k).trim().to_string();
        if !n.starts_with("sso-session") && !n.starts_with("services ") {
            names.push(n);
        }
    }
    names.sort_by(|a, b| (a != "default", a).cmp(&(b != "default", b)));
    names.dedup();
    names
}

/// What a profile resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub credentials: Credentials,
    pub region: Option<String>,
    pub endpoint: Option<String>,
}

pub fn load_profile(requested: &str) -> VfsResult<Profile> {
    resolve_profile(
        &effective_profile(requested),
        &read(credentials_file()),
        &read(config_file()),
    )
}

pub fn resolve_profile(name: &str, creds: &Ini, config: &Ini) -> VfsResult<Profile> {
    let cfg_section = if name == "default" {
        config
            .get("default")
            .or_else(|| config.get("profile default"))
    } else {
        config.get(&format!("profile {name}"))
    };
    let cred_section = creds.get(name);
    if cfg_section.is_none() && cred_section.is_none() {
        return Err(VfsError::Auth(format!(
            "AWS profile “{name}” not found in ~/.aws/credentials or ~/.aws/config"
        )));
    }
    let get = |key: &str| -> Option<String> {
        cred_section
            .and_then(|s| s.get(key))
            .or_else(|| cfg_section.and_then(|s| s.get(key)))
            .filter(|v| !v.is_empty())
            .cloned()
    };
    let (Some(access_key), Some(secret_key)) =
        (get("aws_access_key_id"), get("aws_secret_access_key"))
    else {
        let kind = if get("sso_session").is_some() || get("sso_start_url").is_some() {
            "an SSO profile"
        } else if get("credential_process").is_some() {
            "a credential_process profile"
        } else if get("role_arn").is_some() {
            "an assume-role profile"
        } else {
            "a profile without access keys"
        };
        return Err(VfsError::Auth(format!(
            "AWS profile “{name}” is {kind}; only profiles with aws_access_key_id and \
             aws_secret_access_key are supported (try `aws configure export-credentials`)"
        )));
    };
    Ok(Profile {
        credentials: Credentials {
            access_key,
            secret_key,
            session_token: get("aws_session_token"),
        },
        region: cfg_section
            .and_then(|s| s.get("region"))
            .filter(|r| !r.is_empty())
            .cloned(),
        endpoint: get("endpoint_url"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREDS: &str = "\
[default]
aws_access_key_id = AKIADEFAULT
aws_secret_access_key = secret/default

# comment
[work]
aws_access_key_id=AKIAWORK
aws_secret_access_key=secretwork
aws_session_token = tok
";
    const CONFIG: &str = "\
[default]
region = eu-central-1

[profile work]
region=us-west-2
endpoint_url = https://minio.example.com

[profile sso]
sso_session = corp
region = eu-west-1

[sso-session corp]
sso_start_url = https://corp.awsapps.com/start
";

    #[test]
    fn resolves_profiles() {
        let (c, f) = (parse_ini(CREDS), parse_ini(CONFIG));
        let d = resolve_profile("default", &c, &f).unwrap();
        assert_eq!(d.credentials.access_key, "AKIADEFAULT");
        assert_eq!(d.credentials.secret_key, "secret/default");
        assert_eq!(d.region.as_deref(), Some("eu-central-1"));
        let w = resolve_profile("work", &c, &f).unwrap();
        assert_eq!(w.credentials.session_token.as_deref(), Some("tok"));
        assert_eq!(w.region.as_deref(), Some("us-west-2"));
        assert_eq!(w.endpoint.as_deref(), Some("https://minio.example.com"));
    }

    #[test]
    fn unsupported_and_missing_profiles() {
        let (c, f) = (parse_ini(CREDS), parse_ini(CONFIG));
        match resolve_profile("sso", &c, &f) {
            Err(VfsError::Auth(m)) => assert!(m.contains("SSO"), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            resolve_profile("nope", &c, &f),
            Err(VfsError::Auth(_))
        ));
    }

    #[test]
    fn lists_profile_names() {
        let names = profile_names(&parse_ini(CREDS), &parse_ini(CONFIG));
        assert_eq!(names, ["default", "sso", "work"]);
    }
}
