//! "Report an Issue…" / "Suggest an Idea…" (Help menu, palette, Settings ›
//! About Manta): GitHub's issue forms (`.github/ISSUE_TEMPLATE/*.yml`) in
//! the browser, the bug report prefilled with the Manta version, the macOS
//! version and the architecture (GitHub fills form fields from query
//! parameters named after their `id`). Nothing is sent by Manta itself:
//! the user sees the form and submits it.

/// The repository's "new issue" page.
pub const NEW_ISSUE_URL: &str = "https://github.com/web-workers-com/manta/issues/new";

/// The issue chooser (bug report, idea, manual, security).
pub const CHOOSE_URL: &str = "https://github.com/web-workers-com/manta/issues/new/choose";

/// What the user wants to tell us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `bug_report.yml`.
    Bug,
    /// `feature_request.yml`.
    Idea,
}

/// The system details prefilled in a bug report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    /// e.g. `0.3.4`.
    pub manta: String,
    /// e.g. `macOS 15.6.1`.
    pub macos: String,
    /// e.g. `Apple silicon (arm64)`.
    pub arch: String,
}

impl SystemInfo {
    /// This build on this Mac.
    pub fn current() -> Self {
        Self {
            manta: env!("CARGO_PKG_VERSION").to_string(),
            macos: macos_version(),
            arch: arch_label(std::env::consts::ARCH),
        }
    }
}

/// The issue form for `kind`, a bug report prefilled from `info`.
pub fn issue_url(kind: Kind, info: &SystemInfo) -> String {
    match kind {
        Kind::Bug => format!(
            "{NEW_ISSUE_URL}?template=bug_report.yml&manta-version={}&macos-version={}&mac-model={}",
            encode(&info.manta),
            encode(&info.macos),
            encode(&info.arch),
        ),
        Kind::Idea => format!("{NEW_ISSUE_URL}?template=feature_request.yml"),
    }
}

/// URLs longer than this are cut by browsers and GitHub (8 kB is safe).
pub const MAX_URL: usize = 8000;

/// The bug form after a crash: the system details, a title and the
/// "Actual behaviour" text. When the URL would be longer than [`MAX_URL`],
/// `shorten(budget)` is asked for a shorter text (`budget`: characters of
/// the summary to keep), smaller and smaller until it fits.
pub fn crash_issue_url(
    info: &SystemInfo,
    title: &str,
    actual: &str,
    shorten: impl Fn(usize) -> String,
) -> String {
    let url = |actual: &str| {
        format!(
            "{}&title={}&actual={}",
            issue_url(Kind::Bug, info),
            encode(title),
            encode(actual)
        )
    };
    let mut out = url(actual);
    let mut budget = actual.chars().count();
    while out.len() > MAX_URL && budget > 0 {
        budget = budget * 3 / 4;
        out = url(&shorten(budget));
    }
    out
}

/// [`issue_url`] for this build on this Mac.
pub fn current_issue_url(kind: Kind) -> String {
    match kind {
        Kind::Bug => issue_url(kind, &SystemInfo::current()),
        // Nothing to prefill: don't ask the system.
        Kind::Idea => issue_url(
            kind,
            &SystemInfo {
                manta: String::new(),
                macos: String::new(),
                arch: String::new(),
            },
        ),
    }
}

/// `aarch64` → "Apple silicon (arm64)", `x86_64` → "Intel (x86_64)".
pub fn arch_label(arch: &str) -> String {
    match arch {
        "aarch64" => "Apple silicon (arm64)".into(),
        "x86_64" => "Intel (x86_64)".into(),
        other => other.into(),
    }
}

/// "macOS 15.6.1" (elsewhere: the OS name).
fn macos_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let v = objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion();
        format_macos(v.majorVersion, v.minorVersion, v.patchVersion)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::consts::OS.to_string()
    }
}

/// "macOS 15.6" / "macOS 15.6.1" (no ".0" patch level).
pub fn format_macos(major: isize, minor: isize, patch: isize) -> String {
    if patch > 0 {
        format!("macOS {major}.{minor}.{patch}")
    } else {
        format!("macOS {major}.{minor}")
    }
}

/// Percent-encode a query parameter value (RFC 3986 unreserved kept).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> SystemInfo {
        SystemInfo {
            manta: "0.3.4".into(),
            macos: "macOS 15.6.1".into(),
            arch: "Apple silicon (arm64)".into(),
        }
    }

    #[test]
    fn bug_report_is_prefilled() {
        assert_eq!(
            issue_url(Kind::Bug, &info()),
            "https://github.com/web-workers-com/manta/issues/new?template=bug_report.yml\
             &manta-version=0.3.4&macos-version=macOS%2015.6.1\
             &mac-model=Apple%20silicon%20%28arm64%29"
        );
    }

    #[test]
    fn idea_opens_the_feature_form() {
        assert_eq!(
            issue_url(Kind::Idea, &info()),
            "https://github.com/web-workers-com/manta/issues/new?template=feature_request.yml"
        );
    }

    /// The prefilled fields exist in the issue form (GitHub ignores
    /// unknown ones silently).
    #[test]
    fn field_ids_match_the_issue_form() {
        let form = include_str!("../../../.github/ISSUE_TEMPLATE/bug_report.yml");
        for id in ["manta-version", "macos-version", "mac-model"] {
            assert!(form.contains(&format!("id: {id}\n")), "{id} missing");
        }
        let idea = include_str!("../../../.github/ISSUE_TEMPLATE/feature_request.yml");
        assert!(idea.starts_with("name: "));
    }

    #[test]
    fn current_system() {
        let s = SystemInfo::current();
        assert_eq!(s.manta, env!("CARGO_PKG_VERSION"));
        assert!(!s.macos.is_empty());
        #[cfg(target_os = "macos")]
        assert!(s.macos.starts_with("macOS 1") || s.macos.starts_with("macOS 2"));
        assert!(current_issue_url(Kind::Bug).contains("&manta-version="));
    }

    #[test]
    fn helpers() {
        assert_eq!(arch_label("aarch64"), "Apple silicon (arm64)");
        assert_eq!(arch_label("x86_64"), "Intel (x86_64)");
        assert_eq!(format_macos(15, 6, 0), "macOS 15.6");
        assert_eq!(format_macos(26, 0, 1), "macOS 26.0.1");
        assert_eq!(encode("a b&c=d/ü"), "a%20b%26c%3Dd%2F%C3%BC");
    }
}
