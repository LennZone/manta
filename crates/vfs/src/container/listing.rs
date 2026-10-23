//! Listing files inside a container, and reading the tar streams of
//! `docker cp`.
//!
//! Listing runs one portable POSIX `sh` script ([`LIST_SCRIPT`]) that works
//! with GNU coreutils and with busybox (alpine). Names are never put into
//! the script: the path is a positional argument, and every name is sent
//! back NUL-terminated, so spaces, quotes and even newlines survive.
//!
//! Output: one record per entry, `name \0 kind[L target] \0`, then an
//! empty field (`\0`), then one `size mtime rawmode-hex` line per entry
//! from `stat -L` in the same order (`? ? ?` where `stat` failed or is
//! missing).

use std::io::{self, Read};

use manta_core::{Entry, EntryKind, VfsError, VfsPath, VfsResult};

use crate::unix_to_utc;

/// `sh -c LIST_SCRIPT sh MODE PATH`. MODE `L` lists the directory PATH,
/// MODE `S` reports PATH itself.
pub const LIST_SCRIPT: &str = r#"m=$1; p=$2
if [ "$m" = S ]; then
  [ -e "$p" ] || [ -L "$p" ] || { echo "$p: No such file or directory" >&2; exit 2; }
  set -- "$p"
else
  if [ ! -d "$p" ]; then
    if [ -e "$p" ]; then echo "$p: Not a directory" >&2; else echo "$p: No such file or directory" >&2; fi
    exit 2
  fi
  cd "$p" || exit 1
  set --
  for f in .* *; do
    case $f in .|..) continue;; esac
    [ -e "$f" ] || [ -L "$f" ] || continue
    set -- "$@" "./$f"
  done
fi
for f in "$@"; do
  if [ -d "$f" ]; then k=d; elif [ -f "$f" ]; then k=f; else k=o; fi
  if [ -L "$f" ]; then printf '%s\0%sL%s\0' "$f" "$k" "$(readlink "$f" 2>/dev/null)"; else printf '%s\0%s\0' "$f" "$k"; fi
done
printf '\0'
[ $# -gt 0 ] || exit 0
if s=$(stat -L -c '%s %Y %f' "$@" 2>/dev/null); then
  printf '%s\n' "$s"
else
  for f in "$@"; do stat -L -c '%s %Y %f' "$f" 2>/dev/null || stat -c '%s %Y %f' "$f" 2>/dev/null || echo '? ? ?'; done
fi
"#;

/// Parse the output of [`LIST_SCRIPT`]. `dir` is the listed directory
/// (MODE `L`) or the parent of the stat'ed path (MODE `S`, where names are
/// absolute).
pub fn parse_listing(out: &[u8], dir: &VfsPath) -> VfsResult<Vec<Entry>> {
    let bad = |what: &str| VfsError::Other(format!("unexpected listing output ({what})"));
    let mut fields = out.split(|b| *b == 0);
    let mut records: Vec<(String, u8, Option<String>)> = Vec::new();
    loop {
        let name = fields.next().ok_or_else(|| bad("missing end marker"))?;
        if name.is_empty() {
            break;
        }
        let info = fields.next().ok_or_else(|| bad("missing kind"))?;
        let name = String::from_utf8_lossy(name);
        let name = name.strip_prefix("./").unwrap_or(&name).to_string();
        let kind = *info.first().ok_or_else(|| bad("empty kind"))?;
        let link =
            (info.get(1) == Some(&b'L')).then(|| String::from_utf8_lossy(&info[2..]).into_owned());
        records.push((name, kind, link));
    }
    // Everything after the end marker: the stat lines.
    let rest: Vec<u8> = {
        let mut v = Vec::new();
        let mut first = true;
        for f in fields {
            if !first {
                v.push(0);
            }
            v.extend_from_slice(f);
            first = false;
        }
        v
    };
    let text = String::from_utf8_lossy(&rest);
    let stats: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut entries = Vec::with_capacity(records.len());
    for (i, (name, kind, link)) in records.into_iter().enumerate() {
        let (size, mtime, mode) = stats
            .get(i)
            .map(|l| parse_stat_line(l))
            .unwrap_or((None, None, None));
        let (path, name) = if name.starts_with('/') {
            let p = VfsPath::new(&name);
            let n = if p.is_root() {
                "/".to_string()
            } else {
                p.file_name().to_string()
            };
            (p, n)
        } else {
            (dir.join(&name), name)
        };
        let kind = match kind {
            // Like the local backend: a link to a folder can be opened.
            b'd' => EntryKind::Directory,
            _ if link.is_some() => EntryKind::Symlink,
            b'f' => EntryKind::File,
            _ => EntryKind::Other,
        };
        entries.push(Entry {
            hidden: name.starts_with('.'),
            name,
            path,
            size: if kind == EntryKind::Directory {
                0
            } else {
                size.unwrap_or(0)
            },
            modified: mtime.and_then(unix_to_utc),
            permissions: mode.map(|m| m & 0o7777),
            link_target: link,
            kind,
        });
    }
    Ok(entries)
}

/// `"4096 1792526400 41ed"` → (size, mtime, mode).
pub fn parse_stat_line(l: &str) -> (Option<u64>, Option<i64>, Option<u32>) {
    let mut it = l.split_whitespace();
    let size = it.next().and_then(|s| s.parse().ok());
    let mtime = it.next().and_then(|s| s.parse().ok());
    let mode = it.next().and_then(|s| u32::from_str_radix(s, 16).ok());
    (size, mtime, mode)
}

/// POSIX shell quoting for commands shown to (or typed for) the user.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,@%:=".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

// ------------------------------------------------------------------ tar

/// At most this much tar data is read to list a folder of a stopped
/// container (`docker cp` sends the whole subtree).
pub const TAR_LIST_LIMIT: u64 = 512 * 1024 * 1024;

fn entry_kind(t: tar::EntryType) -> EntryKind {
    if t.is_dir() {
        EntryKind::Directory
    } else if t.is_symlink() {
        EntryKind::Symlink
    } else if t.is_file() || t.is_hard_link() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// Entry of a tar header (`path` is where it lives in the VFS).
fn tar_entry(h: &tar::Header, path: VfsPath, link: Option<String>) -> Entry {
    let kind = entry_kind(h.entry_type());
    let name = if path.is_root() {
        "/".to_string()
    } else {
        path.file_name().to_string()
    };
    Entry {
        hidden: name.starts_with('.'),
        name,
        size: if kind == EntryKind::File {
            h.size().unwrap_or(0)
        } else {
            0
        },
        modified: h.mtime().ok().and_then(|t| unix_to_utc(t as i64)),
        permissions: h.mode().ok().map(|m| m & 0o7777),
        link_target: link,
        kind,
        path,
    }
}

/// Counts bytes and refuses to read more than `limit`.
struct Limited<R> {
    inner: R,
    read: u64,
    limit: u64,
}

impl<R: Read> Read for Limited<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.read > self.limit {
            return Err(io::Error::other("too large"));
        }
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

/// List `dir` from the tar stream of `docker cp ctr:dir -`: the first
/// entry is `dir` itself, its direct children follow (named
/// `<first>/<child>`). Deeper entries are skipped.
pub fn list_tar<R: Read>(r: R, dir: &VfsPath, limit: u64) -> VfsResult<Vec<Entry>> {
    let mut ar = tar::Archive::new(Limited {
        inner: r,
        read: 0,
        limit,
    });
    let mut root: Option<String> = None;
    let mut out = Vec::new();
    let entries = ar
        .entries()
        .map_err(|e| VfsError::Other(format!("reading tar: {e}")))?;
    for e in entries {
        let e = match e {
            Ok(e) => e,
            Err(err) if err.to_string().contains("too large") => {
                return Err(VfsError::Unsupported(
                    "this folder is too large to list while the container is stopped — start the container to browse it".into(),
                ))
            }
            Err(err) => return Err(VfsError::Other(format!("reading tar: {err}"))),
        };
        let raw = String::from_utf8_lossy(&e.path_bytes()).into_owned();
        // `d/x` (docker cp of /d), `/x` (of /), `./x` (other tar writers).
        let p = raw.trim_end_matches('/');
        let p = p.strip_prefix("./").unwrap_or(p);
        let p = if p == "." {
            String::new()
        } else {
            p.to_string()
        };
        let Some(base) = &root else {
            if !e.header().entry_type().is_dir() {
                return Err(VfsError::NotADirectory(dir.to_string()));
            }
            root = Some(p);
            continue;
        };
        let rest = if base.is_empty() {
            p.trim_start_matches('/')
        } else {
            match p.strip_prefix(base.as_str()) {
                Some(r) => r.trim_start_matches('/'),
                None => continue,
            }
        };
        if rest.is_empty() || rest.contains('/') {
            continue;
        }
        let link = e
            .link_name_bytes()
            .map(|l| String::from_utf8_lossy(&l).into_owned());
        out.push(tar_entry(e.header(), dir.join(rest), link));
    }
    if root.is_none() {
        return Err(VfsError::NotFound(dir.to_string()));
    }
    Ok(out)
}

/// Read one 512-byte block; `Ok(false)` at a clean end of stream.
fn read_block<R: Read>(r: &mut R, buf: &mut [u8; 512]) -> io::Result<bool> {
    let mut got = 0;
    while got < 512 {
        let n = r.read(&mut buf[got..])?;
        if n == 0 {
            return if got == 0 {
                Ok(false)
            } else {
                Err(io::ErrorKind::UnexpectedEof.into())
            };
        }
        got += n;
    }
    Ok(true)
}

fn skip<R: Read>(r: &mut R, n: u64) -> io::Result<()> {
    let copied = io::copy(&mut r.take(n), &mut io::sink())?;
    if copied < n {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(())
}

fn padded(n: u64) -> u64 {
    n.div_ceil(512) * 512
}

/// `size` from PAX extended header records (`"23 size=12345678901\n"`).
fn pax_size(data: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(data);
    text.lines().find_map(|l| {
        l.split_once(' ')?
            .1
            .strip_prefix("size=")?
            .trim()
            .parse()
            .ok()
    })
}

/// Read the first real header of a tar stream (skipping PAX and GNU long
/// name headers). `None` for an empty archive.
pub fn first_header<R: Read>(r: &mut R) -> io::Result<Option<(tar::Header, u64, Option<String>)>> {
    let mut block = [0u8; 512];
    let mut size_override = None;
    let mut long_name = None;
    loop {
        if !read_block(r, &mut block)? {
            return Ok(None);
        }
        if block.iter().all(|b| *b == 0) {
            return Ok(None);
        }
        let h = tar::Header::from_byte_slice(&block).clone();
        let size = h.entry_size()?;
        match h.entry_type() {
            tar::EntryType::XHeader
            | tar::EntryType::XGlobalHeader
            | tar::EntryType::GNULongName
            | tar::EntryType::GNULongLink => {
                let mut data = vec![0u8; size as usize];
                r.read_exact(&mut data)?;
                skip(r, padded(size) - size)?;
                match h.entry_type() {
                    tar::EntryType::XHeader => size_override = pax_size(&data).or(size_override),
                    tar::EntryType::GNULongName => {
                        long_name = Some(
                            String::from_utf8_lossy(&data)
                                .trim_end_matches('\0')
                                .to_string(),
                        )
                    }
                    _ => {}
                }
            }
            _ => {
                let size = size_override.unwrap_or(size);
                return Ok(Some((h, size, long_name)));
            }
        }
    }
}

/// Streams the content of the single file in a `docker cp ctr:file -` tar.
pub struct TarFileReader<R> {
    inner: R,
    remaining: u64,
}

impl<R: Read> TarFileReader<R> {
    /// Read the header; fails if the tar's first entry isn't a regular file.
    pub fn new(mut inner: R, path: &VfsPath) -> VfsResult<TarFileReader<R>> {
        let (h, size, _) = first_header(&mut inner)
            .map_err(|e| VfsError::Other(format!("{path}: reading tar: {e}")))?
            .ok_or_else(|| VfsError::NotFound(path.to_string()))?;
        let t = h.entry_type();
        if t.is_dir() {
            return Err(VfsError::Other(format!("{path} is a folder")));
        }
        if !(t.is_file() || t == tar::EntryType::Continuous) {
            return Err(VfsError::Unsupported(format!(
                "{path} is not a regular file"
            )));
        }
        Ok(TarFileReader {
            inner,
            remaining: size,
        })
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for TarFileReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let want = buf
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "tar stream ended early",
            ));
        }
        self.remaining -= n as u64;
        Ok(n)
    }
}

/// The first entry of a `docker cp ctr:path -` tar as an [`Entry`] (stat
/// of a stopped container's file).
pub fn stat_tar<R: Read>(mut r: R, path: &VfsPath) -> VfsResult<Entry> {
    let (h, size, _) = first_header(&mut r)
        .map_err(|e| VfsError::Other(format!("{path}: reading tar: {e}")))?
        .ok_or_else(|| VfsError::NotFound(path.to_string()))?;
    let link = h
        .link_name_bytes()
        .map(|l| String::from_utf8_lossy(&l).into_owned());
    let mut e = tar_entry(&h, path.clone(), link);
    if e.kind == EntryKind::File {
        e.size = size;
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the script prints, built by hand.
    fn script_output(records: &[(&str, &str)], stats: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for (n, k) in records {
            v.extend_from_slice(n.as_bytes());
            v.push(0);
            v.extend_from_slice(k.as_bytes());
            v.push(0);
        }
        v.push(0);
        for s in stats {
            v.extend_from_slice(s.as_bytes());
            v.push(b'\n');
        }
        v
    }

    #[test]
    fn parses_gnu_and_busybox_listing_with_odd_names() {
        let dir = VfsPath::new("/srv/app");
        let out = script_output(
            &[
                ("./plain.txt", "f"),
                ("./with space's \"quote\".txt", "f"),
                ("./line\nbreak", "f"),
                ("./.env", "f"),
                ("./lib", "d"),
                ("./current", "dLreleases/42"),
                ("./dangling", "oL/nowhere"),
                ("./fifo", "o"),
            ],
            // GNU stat (directory size 4096) and busybox (same format).
            &[
                "12 1792526400 81a4",
                "0 1792526400 81a4",
                "3 1700000000 8180",
                "77 1700000000 8180",
                "4096 1792526400 41ed",
                "4096 1792526400 41ed",
                "9 1700000000 a1ff",
                "0 1700000000 1180",
            ],
        );
        let e = parse_listing(&out, &dir).unwrap();
        assert_eq!(e.len(), 8);
        assert_eq!(
            (e[0].name.as_str(), e[0].size, e[0].kind),
            ("plain.txt", 12, EntryKind::File)
        );
        assert_eq!(e[0].permissions, Some(0o644));
        assert_eq!(e[0].modified.unwrap().timestamp(), 1792526400);
        assert_eq!(e[1].name, "with space's \"quote\".txt");
        assert_eq!(e[1].path.as_str(), "/srv/app/with space's \"quote\".txt");
        assert_eq!(e[2].name, "line\nbreak");
        assert_eq!(e[2].permissions, Some(0o600));
        assert!(e[3].hidden);
        assert_eq!((e[4].kind, e[4].size), (EntryKind::Directory, 0));
        // A link to a folder can be opened; it keeps its target.
        assert_eq!(
            (e[5].kind, e[5].link_target.as_deref()),
            (EntryKind::Directory, Some("releases/42"))
        );
        assert_eq!(
            (e[6].kind, e[6].link_target.as_deref()),
            (EntryKind::Symlink, Some("/nowhere"))
        );
        assert_eq!(e[7].kind, EntryKind::Other);
    }

    #[test]
    fn missing_stat_falls_back_to_unknown_sizes() {
        let out = script_output(&[("./a", "f"), ("./b", "d")], &["? ? ?", "? ? ?"]);
        let e = parse_listing(&out, &VfsPath::new("/")).unwrap();
        assert_eq!(
            (e[0].size, e[0].modified, e[0].permissions),
            (0, None, None)
        );
        assert_eq!(e[0].path.as_str(), "/a");
        assert!(e[1].is_dir());
    }

    #[test]
    fn empty_dir_and_stat_mode() {
        assert!(parse_listing(b"\0", &VfsPath::new("/empty"))
            .unwrap()
            .is_empty());
        let out = script_output(&[("/etc/hosts", "f")], &["174 1700000000 81a4"]);
        let e = parse_listing(&out, &VfsPath::new("/etc")).unwrap();
        assert_eq!(
            (e[0].name.as_str(), e[0].path.as_str(), e[0].size),
            ("hosts", "/etc/hosts", 174)
        );
        let root = script_output(&[("/", "d")], &["4096 1 41ed"]);
        assert_eq!(
            parse_listing(&root, &VfsPath::root()).unwrap()[0].path,
            VfsPath::root()
        );
        assert!(
            parse_listing(b"x\0", &VfsPath::root()).is_err(),
            "truncated output"
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/var/www"), "/var/www");
        assert_eq!(sh_quote("my file"), "'my file'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("$(rm -rf /)"), "'$(rm -rf /)'");
    }

    fn tar_of(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            match data {
                Some(d) => {
                    h.set_entry_type(tar::EntryType::Regular);
                    h.set_size(d.len() as u64);
                    h.set_mode(0o640);
                    h.set_mtime(1_700_000_000);
                    b.append_data(&mut h, name, *d).unwrap();
                }
                None => {
                    h.set_entry_type(tar::EntryType::Directory);
                    h.set_size(0);
                    h.set_mode(0o755);
                    b.append_data(&mut h, name, io::empty()).unwrap();
                }
            }
        }
        b.into_inner().unwrap()
    }

    #[test]
    fn tar_listing_keeps_direct_children() {
        let data = tar_of(&[
            ("d/", None),
            ("d/a.txt", Some(b"hi")),
            ("d/sub/", None),
            ("d/sub/deep.txt", Some(b"no")),
            ("d/name with space", Some(b"x")),
        ]);
        let e = list_tar(&data[..], &VfsPath::new("/tmp/d"), TAR_LIST_LIMIT).unwrap();
        let names: Vec<_> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "sub", "name with space"]);
        assert_eq!(e[0].path.as_str(), "/tmp/d/a.txt");
        assert_eq!((e[0].size, e[0].permissions), (2, Some(0o640)));
        assert!(e[1].is_dir());
        // A size cap gives a clear message.
        let err = list_tar(&data[..], &VfsPath::new("/tmp/d"), 10).unwrap_err();
        assert!(err.to_string().contains("too large to list"), "{err}");
        // The first entry being a file: not a folder.
        let f = tar_of(&[("a.txt", Some(b"hi"))]);
        assert!(matches!(
            list_tar(&f[..], &VfsPath::new("/a.txt"), TAR_LIST_LIMIT),
            Err(VfsError::NotADirectory(_))
        ));
    }

    #[test]
    fn tar_reader_streams_single_file() {
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let big = tar_of(&[("big.bin", Some(&body))]);
        let mut r = TarFileReader::new(&big[..], &VfsPath::new("/big.bin")).unwrap();
        let mut got = Vec::new();
        r.read_to_end(&mut got).unwrap();
        assert_eq!(got, body);
        // Long names use a GNU long-name header first.
        let long = format!("{}.txt", "n".repeat(150));
        let data = tar_of(&[(&long, Some(b"long"))]);
        let mut got = String::new();
        TarFileReader::new(&data[..], &VfsPath::new("/x"))
            .unwrap()
            .read_to_string(&mut got)
            .unwrap();
        assert_eq!(got, "long");
        // A folder is refused.
        let d = tar_of(&[("d/", None)]);
        assert!(TarFileReader::new(&d[..], &VfsPath::new("/d")).is_err());
        // Stat from the first header.
        let s = stat_tar(
            &tar_of(&[("a.txt", Some(b"abc"))])[..],
            &VfsPath::new("/x/a.txt"),
        )
        .unwrap();
        assert_eq!(
            (s.name.as_str(), s.size, s.kind),
            ("a.txt", 3, EntryKind::File)
        );
        // A truncated stream is an error, not a short file.
        let mut cut = TarFileReader::new(&big[..1500], &VfsPath::new("/x")).unwrap();
        assert!(cut.read_to_end(&mut Vec::new()).is_err());
    }

    #[test]
    fn pax_size_record() {
        assert_eq!(
            pax_size(b"23 size=12345678901\n20 path=a/b/c.txt\n"),
            Some(12345678901)
        );
        assert_eq!(pax_size(b"20 path=a/b/c.txt\n"), None);
    }
}
