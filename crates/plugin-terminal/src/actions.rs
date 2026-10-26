//! Formatting GUI file actions as the equivalent shell commands (pure).
//!
//! The terminal shows what the app did, e.g. `→ cp -R 'a.txt' '../Design/'`
//! and, once it finished, `✓ 3 files · 12.4 MB · 0.4 s`. Commands are
//! copy-pasteable: paths are quoted with [`crate::escape::quote`] and made
//! relative to the shell's directory where that is shorter. Remote paths use
//! the scp notation `user@host:'/path'`.

use std::path::Path;

use manta_core::{Protocol, VfsPath};
use manta_plugin::i18n;
use manta_plugin::{FileAction, FileActionKind, FileActionPhase, RemoteEndpoint, SyncSpec};

use crate::escape::{quote, ShellKind};
use crate::follow::Dir;
use crate::i18n::tr;

/// At most this many source paths are written out; the rest is summarised
/// as `… (+N more)`.
pub const MAX_LISTED: usize = 3;
/// Error texts are cut to this many characters.
const MAX_ERROR: usize = 160;

/// Where the command is shown: the shell's directory and family.
#[derive(Debug, Clone, Copy)]
pub struct ShellCtx<'a> {
    pub cwd: Option<&'a Dir>,
    pub kind: ShellKind,
}

impl<'a> ShellCtx<'a> {
    pub fn new(cwd: Option<&'a Dir>, kind: ShellKind) -> Self {
        ShellCtx { cwd, kind }
    }
}

// ------------------------------------------------------------------ paths

/// `path` relative to `base` (both absolute, `/`-separated), going up at
/// most two levels. `None` if that is not possible.
fn relative_to(path: &str, base: &str) -> Option<String> {
    let path = trim_slash(path);
    let mut base = trim_slash(base).to_owned();
    let mut ups = 0;
    loop {
        if path == base {
            return Some(if ups == 0 {
                ".".into()
            } else {
                vec![".."; ups].join("/")
            });
        }
        let prefix = if base == "/" {
            "/".to_owned()
        } else {
            format!("{base}/")
        };
        if let Some(rest) = path.strip_prefix(&prefix) {
            let mut s = "../".repeat(ups);
            s.push_str(rest);
            return Some(s);
        }
        if ups == 2 || base == "/" {
            return None;
        }
        base = match base.rfind('/') {
            Some(0) => "/".into(),
            Some(i) => base[..i].to_owned(),
            None => return None,
        };
        ups += 1;
    }
}

fn trim_slash(s: &str) -> &str {
    if s.len() > 1 {
        s.trim_end_matches('/')
    } else {
        s
    }
}

/// The shortest way to write the local `path` for a shell in `cwd`
/// (unquoted). Compares both the spelled and the canonical forms, so
/// `/tmp/x` works for a shell in `/private/tmp`.
pub fn local_display(path: &str, cwd: Option<&Dir>) -> String {
    let path = trim_slash(path).to_owned();
    let Some(cwd) = cwd else { return path };
    let mut candidates = vec![path.clone()];
    // The canonical form of the parent (the item itself may be gone).
    let p = Path::new(&path);
    if let (Some(parent), Some(name)) = (p.parent(), p.file_name()) {
        if let Ok(c) = std::fs::canonicalize(parent) {
            candidates.push(c.join(name).to_string_lossy().into_owned());
        }
    }
    if let Ok(c) = std::fs::canonicalize(p) {
        candidates.push(c.to_string_lossy().into_owned());
    }
    let bases = [
        cwd.raw.to_string_lossy().into_owned(),
        cwd.canon.to_string_lossy().into_owned(),
    ];
    let mut best = path.clone();
    for c in &candidates {
        for b in &bases {
            if let Some(r) = relative_to(c, b) {
                if r.chars().count() < best.chars().count() {
                    best = r;
                }
            }
        }
    }
    best
}

/// Quote a display path; `.`, `..`, `../..` stay bare.
fn q(s: &str, kind: ShellKind) -> String {
    if s.split('/').all(|c| c == "." || c == "..") && !s.is_empty() {
        s.to_owned()
    } else {
        quote(s, kind)
    }
}

/// `user@host` (brackets around IPv6 literals).
fn user_host(ep: &RemoteEndpoint) -> String {
    let host = if ep.host.contains(':') {
        format!("[{}]", ep.host)
    } else {
        ep.host.clone()
    };
    if ep.user.is_empty() {
        host
    } else {
        format!("{}@{host}", ep.user)
    }
}

fn is_default_port(ep: &RemoteEndpoint) -> bool {
    match ep.protocol {
        Protocol::Sftp => ep.port == 22 || ep.port == 0,
        Protocol::Ftp | Protocol::Ftps => ep.port == 21 || ep.port == 0,
        Protocol::S3 | Protocol::WebDavs => ep.port == 443 || ep.port == 0,
        Protocol::WebDav => ep.port == 80 || ep.port == 0,
        Protocol::Smb => ep.port == 445 || ep.port == 0,
        Protocol::Local | Protocol::Docker | Protocol::Kubernetes | Protocol::GoogleDrive => true,
    }
}

/// One side of a command: a local path (relative where shorter) or a
/// remote `user@host:'/path'`.
struct Side<'a> {
    remote: Option<&'a RemoteEndpoint>,
}

impl Side<'_> {
    /// (raw text for the `--` check, rendered argument)
    fn arg(&self, path: &VfsPath, dir: bool, ctx: ShellCtx) -> (String, String) {
        let slash = |s: String| {
            if dir && !s.ends_with('/') && s != "." && !s.ends_with("..") {
                s + "/"
            } else {
                s
            }
        };
        match self.remote {
            Some(ep) => {
                let p = slash(path.as_str().to_owned());
                (
                    String::new(),
                    format!("{}:{}", user_host(ep), quote(&p, ctx.kind)),
                )
            }
            None => {
                let d = local_display(path.as_str(), ctx.cwd);
                let raw = d.clone();
                (raw, q(&slash(d), ctx.kind))
            }
        }
    }
}

/// Sources (at most [`MAX_LISTED`], then `… (+N more)`), rendered.
fn source_args(a: &FileAction, side: &Side, ctx: ShellCtx) -> (Vec<String>, Vec<String>) {
    let mut raws = Vec::new();
    let mut args = Vec::new();
    for p in a.sources.iter().take(MAX_LISTED) {
        let (r, s) = side.arg(p, false, ctx);
        raws.push(r);
        args.push(s);
    }
    if a.sources.len() > MAX_LISTED {
        args.push(more_note(a.sources.len() - MAX_LISTED));
    }
    (raws, args)
}

/// `cmd [--] args…`: adds `--` if a (local) argument starts with `-`.
fn join(cmd: &str, raws: &[String], args: &[String]) -> String {
    let dashdash = raws.iter().any(|r| r.starts_with('-'));
    let mut s = cmd.to_owned();
    if dashdash {
        s.push_str(" --");
    }
    for a in args {
        s.push(' ');
        s.push_str(a);
    }
    s
}

/// `ssh [-p N] user@host "remote command"`.
fn ssh(ep: &RemoteEndpoint, remote_cmd: &str, kind: ShellKind) -> String {
    let port = if is_default_port(ep) {
        String::new()
    } else {
        format!(" -p {}", ep.port)
    };
    let quoted = if remote_cmd
        .chars()
        .any(|c| "\"$`\\!".contains(c) || c.is_control())
    {
        quote(remote_cmd, kind)
    } else {
        format!("\"{remote_cmd}\"")
    };
    format!("ssh{port} {} {quoted}", user_host(ep))
}

/// Paths for a command run on a remote host (absolute, quoted for the
/// remote POSIX shell).
fn remote_paths(paths: &[VfsPath]) -> String {
    let mut v: Vec<String> = paths
        .iter()
        .take(MAX_LISTED)
        .map(|p| quote(p.as_str(), ShellKind::Other))
        .collect();
    if paths.len() > MAX_LISTED {
        v.push(more_note(paths.len() - MAX_LISTED));
    }
    v.join(" ")
}

/// `-P N` for scp/sftp when a remote side uses a non-default port.
fn scp_port(eps: &[Option<&RemoteEndpoint>]) -> String {
    eps.iter()
        .flatten()
        .find(|ep| !is_default_port(ep))
        .map(|ep| format!(" -P {}", ep.port))
        .unwrap_or_default()
}

// ------------------------------------------------------------------ commands

/// The shell command equivalent to `a` (plain text, no colours).
pub fn command(a: &FileAction, ctx: ShellCtx) -> String {
    let src = Side {
        remote: if a.src_local {
            None
        } else {
            a.src_remote.as_ref()
        },
    };
    let dst = Side {
        remote: if a.dst_local {
            None
        } else {
            a.dst_remote.as_ref()
        },
    };
    if let Some(cmd) = s3_command(a, &src, &dst, ctx) {
        return cmd;
    }
    if let Some(cmd) = container_command(a, &src, &dst, ctx) {
        return cmd;
    }
    let src_remote = !a.src_local;
    let dst_remote = !a.dst_local;
    let k = ctx.kind;
    match a.kind {
        FileActionKind::Copy | FileActionKind::Move => {
            let Some(dest) = &a.destination else {
                return String::from("# (no destination)");
            };
            let (mut raws, mut args) = source_args(a, &src, ctx);
            let (r, d) = dst.arg(dest, true, ctx);
            raws.push(r);
            args.push(d);
            let mv = a.kind == FileActionKind::Move;
            if !src_remote && !dst_remote {
                return join(if mv { "mv" } else { "cp -R" }, &raws, &args);
            }
            let both = if src_remote && dst_remote { " -3" } else { "" };
            let port = scp_port(&[src.remote, dst.remote]);
            let copy = join(&format!("scp -r{both}{port}"), &raws, &args);
            if !mv {
                return copy;
            }
            let rm = match src.remote {
                Some(ep) => ssh(ep, &format!("rm -r {}", remote_paths(&a.sources)), k),
                None => {
                    let (raws, args) = source_args(a, &src, ctx);
                    join("rm -r", &raws, &args)
                }
            };
            format!("{copy} && {rm}")
        }
        FileActionKind::Delete => match src.remote {
            Some(ep) => ssh(ep, &format!("rm -r {}", remote_paths(&a.sources)), k),
            None => {
                let (raws, args) = source_args(a, &src, ctx);
                join("rm -r", &raws, &args)
            }
        },
        // Only local items go to the Trash; shown as the move it is.
        FileActionKind::Trash => {
            let (mut raws, mut args) = source_args(a, &src, ctx);
            raws.push("~/.Trash/".into());
            args.push("~/.Trash/".into());
            join("mv", &raws, &args)
        }
        FileActionKind::Rename if a.renames.len() > 1 => {
            // Batch rename: one `mv` per pair (at most MAX_LISTED).
            let more = a.renames.len().saturating_sub(MAX_LISTED);
            let pairs = &a.renames[..a.renames.len().min(MAX_LISTED)];
            let mut s = match src.remote {
                Some(ep) => {
                    let mvs: Vec<String> = pairs
                        .iter()
                        .map(|(o, n)| format!("mv {}", remote_paths(&[o.clone(), n.clone()])))
                        .collect();
                    ssh(ep, &mvs.join("; "), k)
                }
                None => pairs
                    .iter()
                    .map(|(o, n)| {
                        let (r1, s1) = src.arg(o, false, ctx);
                        let (r2, s2) = src.arg(n, false, ctx);
                        join("mv", &[r1, r2], &[s1, s2])
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            };
            if more > 0 {
                s.push_str(&format!(" {}", more_note(more)));
            }
            s
        }
        FileActionKind::Rename => {
            let (Some(old), Some(new)) = (a.sources.first(), &a.destination) else {
                return String::from("# (rename)");
            };
            match src.remote {
                Some(ep) => ssh(
                    ep,
                    &format!("mv {}", remote_paths(&[old.clone(), new.clone()])),
                    k,
                ),
                None => {
                    let (r1, s1) = src.arg(old, false, ctx);
                    let (r2, s2) = src.arg(new, false, ctx);
                    join("mv", &[r1, r2], &[s1, s2])
                }
            }
        }
        FileActionKind::NewFolder => {
            let Some(dir) = a.sources.first() else {
                return String::from("# (new folder)");
            };
            match src.remote {
                Some(ep) => ssh(
                    ep,
                    &format!("mkdir {}", remote_paths(std::slice::from_ref(dir))),
                    k,
                ),
                None => {
                    let (r, s) = src.arg(dir, false, ctx);
                    join("mkdir", &[r], &[s])
                }
            }
        }
        FileActionKind::NewFile => {
            let Some(file) = a.sources.first() else {
                return String::from("# (new file)");
            };
            match src.remote {
                Some(ep) => ssh(
                    ep,
                    &format!("touch {}", remote_paths(std::slice::from_ref(file))),
                    k,
                ),
                None => {
                    let (r, s) = src.arg(file, false, ctx);
                    join("touch", &[r], &[s])
                }
            }
        }
        FileActionKind::Open => match src.remote {
            Some(ep) => {
                let Some(f) = a.sources.first() else {
                    return String::from("# (open)");
                };
                let name = f.file_name();
                let port = scp_port(&[Some(ep)]);
                format!(
                    "scp{port} {}:{} /tmp/ && open {}",
                    user_host(ep),
                    quote(f.as_str(), k),
                    quote(&format!("/tmp/{name}"), k)
                )
            }
            None => {
                let (raws, args) = source_args(a, &src, ctx);
                join("open", &raws, &args)
            }
        },
        FileActionKind::Connect => match a.src_remote.as_ref() {
            Some(ep) => connect_cmd(ep),
            None => format!("# connect {}", a.src_label),
        },
        FileActionKind::Disconnect => match a.src_remote.as_ref() {
            Some(ep) => format!("# disconnect {} ({})", user_host(ep), ep.protocol),
            None => format!("# disconnect {}", a.src_label),
        },
        FileActionKind::Sync => {
            let (Some(from), Some(to)) = (a.sources.first(), &a.destination) else {
                return String::from("# (sync)");
            };
            let spec = a.sync.unwrap_or_default();
            sync_cmd(from, &src, to, &dst, spec, ctx)
        }
        FileActionKind::Compress => compress_cmd(a, &src, ctx),
        FileActionKind::Extract => extract_cmd(a, &dst, ctx),
    }
}

// ------------------------------------------------------------------ S3

fn is_s3(side: &Side) -> bool {
    side.remote.is_some_and(|ep| ep.protocol == Protocol::S3)
}

/// ` --endpoint-url …` for S3-compatible services (nothing for AWS).
fn s3_endpoint_flag(ep: &RemoteEndpoint) -> String {
    let host = ep.host.trim();
    if host.is_empty() || host.ends_with(".amazonaws.com") {
        return String::new();
    }
    let local = host == "localhost" || host.starts_with("127.") || host == "::1";
    let http = ep.port == 80 || (local && ep.port != 443);
    let scheme = if http { "http" } else { "https" };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let port = match (http, ep.port) {
        (_, 0) | (true, 80) | (false, 443) => String::new(),
        (_, p) => format!(":{p}"),
    };
    format!(" --endpoint-url {scheme}://{host}{port}")
}

/// Quote an `s3://` URI only when the shell needs it.
fn q_s3(s: &str, kind: ShellKind) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+,@%:~=".contains(c))
    {
        s.to_owned()
    } else {
        quote(s, kind)
    }
}

/// `s3://bucket/key` for a pane path `/bucket/key`.
fn s3_uri(path: &VfsPath, dir: bool) -> String {
    let p = path.as_str().trim_matches('/');
    let mut s = format!("s3://{p}");
    if dir && !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// Guess whether a source is a folder: local paths are checked, S3 keys
/// without an extension are taken as folders (the action doesn't say).
fn looks_like_dir(path: &VfsPath, side: &Side) -> bool {
    match side.remote {
        None => Path::new(path.as_str()).is_dir(),
        Some(_) => !path.file_name().contains('.'),
    }
}

/// One argument on either side of an `aws s3` command.
fn s3_arg(path: &VfsPath, dir: bool, side: &Side, ctx: ShellCtx) -> String {
    if is_s3(side) {
        return q_s3(&s3_uri(path, dir), ctx.kind);
    }
    let (_, arg) = side.arg(path, dir, ctx);
    arg
}

/// AWS CLI equivalents of actions on S3 (`aws s3 cp/mv/rm/sync`, ...).
/// `None` when neither side is S3, or for archive actions.
fn s3_command(a: &FileAction, src: &Side, dst: &Side, ctx: ShellCtx) -> Option<String> {
    if !is_s3(src) && !is_s3(dst) {
        return None;
    }
    let ep = [src.remote, dst.remote]
        .into_iter()
        .flatten()
        .find(|e| e.protocol == Protocol::S3)?;
    let flag = s3_endpoint_flag(ep);
    // A server that isn't S3 on the other side: not a literal command.
    let other_remote = [src.remote, dst.remote]
        .into_iter()
        .flatten()
        .any(|e| e.protocol != Protocol::S3);
    let per_source = |verb: &str, dest: Option<&VfsPath>| -> String {
        let mut cmds: Vec<String> = a
            .sources
            .iter()
            .take(MAX_LISTED)
            .map(|s| {
                let dir = looks_like_dir(s, src);
                let rec = if dir { " --recursive" } else { "" };
                let from = s3_arg(s, false, src, ctx);
                match dest {
                    // Recursive copies take the folder's contents: name the target folder.
                    Some(d) if dir => format!(
                        "aws s3 {verb}{rec}{flag} {from} {}",
                        s3_arg(&d.join(s.file_name()), true, dst, ctx)
                    ),
                    Some(d) => format!("aws s3 {verb}{flag} {from} {}", s3_arg(d, true, dst, ctx)),
                    None => format!("aws s3 {verb}{rec}{flag} {from}"),
                }
            })
            .collect();
        if a.sources.len() > MAX_LISTED {
            cmds.push(more_note(a.sources.len() - MAX_LISTED));
        }
        cmds.join(" && ")
    };
    let cmd = match a.kind {
        FileActionKind::Copy | FileActionKind::Move => {
            let dest = a.destination.as_ref()?;
            per_source(
                if a.kind == FileActionKind::Move {
                    "mv"
                } else {
                    "cp"
                },
                Some(dest),
            )
        }
        FileActionKind::Delete => per_source("rm", None),
        FileActionKind::Rename => {
            let pairs: Vec<(VfsPath, VfsPath)> = if a.renames.is_empty() {
                vec![(a.sources.first()?.clone(), a.destination.clone()?)]
            } else {
                a.renames.clone()
            };
            let mut cmds: Vec<String> = pairs
                .iter()
                .take(MAX_LISTED)
                .map(|(o, n)| {
                    let rec = if looks_like_dir(o, src) {
                        " --recursive"
                    } else {
                        ""
                    };
                    format!(
                        "aws s3 mv{rec}{flag} {} {}",
                        s3_arg(o, false, src, ctx),
                        s3_arg(n, false, src, ctx)
                    )
                })
                .collect();
            if pairs.len() > MAX_LISTED {
                cmds.push(more_note(pairs.len() - MAX_LISTED));
            }
            cmds.join("; ")
        }
        FileActionKind::NewFolder => {
            let dir = a.sources.first()?;
            let comps = dir.components();
            match comps.split_first() {
                Some((bucket, [])) => format!(
                    "aws s3 mb{flag} {}",
                    q_s3(&format!("s3://{bucket}"), ctx.kind)
                ),
                Some((bucket, rest)) => format!(
                    "aws s3api put-object{flag} --bucket {} --key {}",
                    q_s3(bucket, ctx.kind),
                    quote(&format!("{}/", rest.join("/")), ctx.kind)
                ),
                None => return None,
            }
        }
        FileActionKind::NewFile => {
            let comps = a.sources.first()?.components();
            let (bucket, rest) = comps.split_first()?;
            if rest.is_empty() {
                return None;
            }
            format!(
                "aws s3api put-object{flag} --bucket {} --key {}",
                q_s3(bucket, ctx.kind),
                quote(&rest.join("/"), ctx.kind)
            )
        }
        FileActionKind::Open => {
            let f = a.sources.first()?;
            format!(
                "aws s3 cp{flag} {} /tmp/ && open {}",
                q_s3(&s3_uri(f, false), ctx.kind),
                quote(&format!("/tmp/{}", f.file_name()), ctx.kind)
            )
        }
        FileActionKind::Connect => {
            let path = a
                .sources
                .first()
                .map(|p| format!(" {}", q_s3(&s3_uri(p, true), ctx.kind)));
            format!("aws s3 ls{flag}{}", path.unwrap_or_default())
        }
        FileActionKind::Disconnect => return Some(format!("# disconnect {} (S3)", ep.host)),
        FileActionKind::Sync => {
            let (from, to) = (a.sources.first()?, a.destination.as_ref()?);
            let (x, y) = (s3_arg(from, true, src, ctx), s3_arg(to, true, dst, ctx));
            let spec = a.sync.unwrap_or_default();
            if spec.both_ways {
                format!("aws s3 sync{flag} {x} {y} && aws s3 sync{flag} {y} {x}")
            } else {
                let del = if spec.delete_extra { " --delete" } else { "" };
                format!("aws s3 sync{del}{flag} {x} {y}")
            }
        }
        FileActionKind::Compress | FileActionKind::Extract | FileActionKind::Trash => return None,
    };
    Some(if other_remote {
        format!("# {cmd}")
    } else {
        cmd
    })
}

// ------------------------------------------------------------------ containers

fn is_ctr(side: &Side) -> bool {
    side.remote.is_some_and(|ep| ep.protocol.is_container())
}

/// Kubernetes endpoints carry `namespace/pod` as host and the container
/// as user.
fn kube_parts(ep: &RemoteEndpoint) -> (Option<&str>, &str) {
    match ep.host.split_once('/') {
        Some((ns, pod)) => (Some(ns), pod),
        None => (None, ep.host.as_str()),
    }
}

/// `docker exec [-it] web` / `kubectl exec [-it] [-n ns] pod [-c ctr] --`.
fn ctr_exec_prefix(ep: &RemoteEndpoint, tty: bool) -> String {
    let it = if tty { " -it" } else { "" };
    if ep.protocol == Protocol::Docker {
        return format!("docker exec{it} {}", q_s3(&ep.host, ShellKind::Other));
    }
    let (ns, pod) = kube_parts(ep);
    let ns = ns
        .map(|n| format!(" -n {}", q_s3(n, ShellKind::Other)))
        .unwrap_or_default();
    let c = if ep.user.is_empty() {
        String::new()
    } else {
        format!(" -c {}", q_s3(&ep.user, ShellKind::Other))
    };
    format!("kubectl exec{it}{ns} {}{c} --", q_s3(pod, ShellKind::Other))
}

/// `web:/path` (Docker) or `ns/pod:/path` (kubectl cp), quoted if needed.
fn ctr_arg(path: &VfsPath, dir: bool, side: &Side, ctx: ShellCtx) -> String {
    match side.remote {
        Some(ep) if ep.protocol.is_container() => {
            let mut p = path.as_str().to_owned();
            if dir && !p.ends_with('/') {
                p.push('/');
            }
            q_s3(&format!("{}:{p}", ep.host), ctx.kind)
        }
        _ => side.arg(path, dir, ctx).1,
    }
}

/// `docker cp` / `kubectl cp` and `docker exec …` equivalents of actions
/// in a container. `None` when neither side is a container.
fn container_command(a: &FileAction, src: &Side, dst: &Side, ctx: ShellCtx) -> Option<String> {
    if !is_ctr(src) && !is_ctr(dst) {
        return None;
    }
    let ep = [src.remote, dst.remote]
        .into_iter()
        .flatten()
        .find(|e| e.protocol.is_container())?;
    // A server or a second container on the other side: no single command.
    let other_remote = [src.remote, dst.remote]
        .into_iter()
        .flatten()
        .filter(|e| *e != ep)
        .count()
        > 0
        || (is_ctr(src) && is_ctr(dst) && src.remote != dst.remote);
    let kube = ep.protocol == Protocol::Kubernetes;
    let cp = |from: String, to: String| -> String {
        if kube {
            let c = if ep.user.is_empty() {
                String::new()
            } else {
                format!(" -c {}", q_s3(&ep.user, ctx.kind))
            };
            format!("kubectl cp{c} {from} {to}")
        } else {
            format!("docker cp {from} {to}")
        }
    };
    let exec = |cmd: &str| format!("{} {cmd}", ctr_exec_prefix(ep, false));
    let cmd = match a.kind {
        FileActionKind::Copy | FileActionKind::Move => {
            let dest = a.destination.as_ref()?;
            let mut cmds: Vec<String> = a
                .sources
                .iter()
                .take(MAX_LISTED)
                .map(|s| cp(ctr_arg(s, false, src, ctx), ctr_arg(dest, true, dst, ctx)))
                .collect();
            if a.sources.len() > MAX_LISTED {
                cmds.push(more_note(a.sources.len() - MAX_LISTED));
            }
            let mut s = cmds.join(" && ");
            if a.kind == FileActionKind::Move {
                let rm = if is_ctr(src) {
                    exec(&format!("rm -r {}", remote_paths(&a.sources)))
                } else {
                    let (raws, args) = source_args(a, src, ctx);
                    join("rm -r", &raws, &args)
                };
                s = format!("{s} && {rm}");
            }
            s
        }
        FileActionKind::Delete => exec(&format!("rm -r {}", remote_paths(&a.sources))),
        FileActionKind::Rename => {
            let pairs: Vec<(VfsPath, VfsPath)> = if a.renames.is_empty() {
                vec![(a.sources.first()?.clone(), a.destination.clone()?)]
            } else {
                a.renames.clone()
            };
            let mut cmds: Vec<String> = pairs
                .iter()
                .take(MAX_LISTED)
                .map(|(o, n)| exec(&format!("mv {}", remote_paths(&[o.clone(), n.clone()]))))
                .collect();
            if pairs.len() > MAX_LISTED {
                cmds.push(more_note(pairs.len() - MAX_LISTED));
            }
            cmds.join("; ")
        }
        FileActionKind::NewFolder => exec(&format!(
            "mkdir {}",
            remote_paths(std::slice::from_ref(a.sources.first()?))
        )),
        FileActionKind::NewFile => exec(&format!(
            "touch {}",
            remote_paths(std::slice::from_ref(a.sources.first()?))
        )),
        FileActionKind::Open => {
            let f = a.sources.first()?;
            format!(
                "{} && open {}",
                cp(ctr_arg(f, false, src, ctx), "/tmp/".into()),
                quote(&format!("/tmp/{}", f.file_name()), ctx.kind)
            )
        }
        FileActionKind::Connect => format!("{} sh", ctr_exec_prefix(ep, true)),
        FileActionKind::Disconnect => {
            return Some(format!("# disconnect {} ({})", ep.host, ep.protocol))
        }
        FileActionKind::Sync => {
            let (from, to) = (a.sources.first()?, a.destination.as_ref()?);
            format!(
                "# {}",
                cp(ctr_arg(from, true, src, ctx), ctr_arg(to, true, dst, ctx))
            )
        }
        FileActionKind::Compress | FileActionKind::Extract | FileActionKind::Trash => return None,
    };
    Some(if other_remote {
        format!("# {cmd}")
    } else {
        cmd
    })
}

/// `zip -r` / `tar -czf` for a [`FileActionKind::Compress`]. The archive
/// stores every source under its own name, so the command runs in the
/// sources' folder (`-C` / `cd`) when that isn't the shell's directory.
fn compress_cmd(a: &FileAction, src: &Side, ctx: ShellCtx) -> String {
    let Some(spec) = &a.archive else {
        return String::from("# (compress)");
    };
    let k = ctx.kind;
    let parent = a
        .sources
        .first()
        .and_then(|s| s.parent())
        .unwrap_or_else(VfsPath::root);
    let mut names: Vec<String> = a
        .sources
        .iter()
        .take(MAX_LISTED)
        .map(|s| q(s.file_name(), k))
        .collect();
    if a.sources.len() > MAX_LISTED {
        names.push(more_note(a.sources.len() - MAX_LISTED));
    }
    let dashdash = a.sources.iter().any(|s| s.file_name().starts_with('-'));
    let names = format!("{}{}", if dashdash { "-- " } else { "" }, names.join(" "));
    let in_cwd = src.remote.is_none() && local_display(parent.as_str(), ctx.cwd) == ".";
    let archive = if spec.local {
        local_display(spec.path.as_str(), ctx.cwd)
    } else {
        spec.path.as_str().to_owned()
    };
    let level = spec
        .level
        .filter(|l| *l != 6)
        .map(|l| format!(" -{l}"))
        .unwrap_or_default();
    let cmd = if spec.format.eq_ignore_ascii_case("zip") {
        let excl = if spec.exclude_junk {
            " -x '*.DS_Store' '*__MACOSX*'"
        } else {
            ""
        };
        if in_cwd {
            format!("zip -r{level} {} {names}{excl}", q(&archive, k))
        } else {
            // zip has no -C: run it in the sources' folder with an absolute archive path.
            let parent_disp = q(&local_display(parent.as_str(), ctx.cwd), k);
            format!(
                "(cd {parent_disp} && zip -r{level} {} {names}{excl})",
                quote(spec.path.as_str(), k)
            )
        }
    } else {
        let flag = match spec.format.as_str() {
            "tar.gz" => "-czf",
            "tar.xz" => "-cJf",
            "tar.bz2" => "-cjf",
            "tar.zst" => "--zstd -cf",
            _ => "-cf",
        };
        let excl = if spec.exclude_junk {
            " --exclude .DS_Store --exclude __MACOSX"
        } else {
            ""
        };
        let dir = if in_cwd {
            String::new()
        } else {
            format!(" -C {}", q(&local_display(parent.as_str(), ctx.cwd), k))
        };
        format!("tar{excl} {flag} {}{dir} {names}", q(&archive, k))
    };
    if !a.src_local || !spec.local {
        // Built locally from (or uploaded to) a server: not a literal command.
        format!("# {cmd}")
    } else {
        cmd
    }
}

/// `unzip` / `tar -x` for a [`FileActionKind::Extract`]. Empty `sources`
/// = the whole archive.
fn extract_cmd(a: &FileAction, dst: &Side, ctx: ShellCtx) -> String {
    let Some(spec) = &a.archive else {
        return String::from("# (extract)");
    };
    let Some(dest) = &a.destination else {
        return String::from("# (no destination)");
    };
    let k = ctx.kind;
    let archive = if spec.local {
        q(&local_display(spec.path.as_str(), ctx.cwd), k)
    } else {
        quote(spec.path.as_str(), k)
    };
    let (_, to) = dst.arg(dest, true, ctx);
    // Members relative to the archive root; the pane extracts them flat
    // into the destination, like `--strip-components`.
    let depth = a
        .sources
        .first()
        .and_then(|s| s.parent())
        .map_or(0, |p| p.components().len());
    let mut members: Vec<String> = a
        .sources
        .iter()
        .take(MAX_LISTED)
        .map(|s| q(s.as_str().trim_start_matches('/'), k))
        .collect();
    if a.sources.len() > MAX_LISTED {
        members.push(more_note(a.sources.len() - MAX_LISTED));
    }
    let members = if members.is_empty() {
        String::new()
    } else {
        format!(" {}", members.join(" "))
    };
    let fmt = spec.format.as_str();
    let cmd = match fmt {
        "ZIP" | "zip" => {
            let junk = if depth > 0 { " -j" } else { "" };
            format!("unzip{junk} {archive}{members} -d {to}")
        }
        "gzip" | "bzip2" | "xz" | "zstd" => {
            let tool = match fmt {
                "gzip" => "gunzip -c",
                "bzip2" => "bunzip2 -c",
                "xz" => "xz -dc",
                _ => "zstd -dc",
            };
            let name = a
                .sources
                .first()
                .map(|s| s.file_name().to_owned())
                .unwrap_or_default();
            let target = format!("{}/{name}", dest.as_str().trim_end_matches('/'));
            let (_, out) = dst.arg(&VfsPath::new(&target), false, ctx);
            format!("{tool} {archive} > {out}")
        }
        _ => {
            let flag = match fmt {
                "tar.gz" => "-xzf",
                "tar.xz" => "-xJf",
                "tar.bz2" => "-xjf",
                "tar.zst" => "--zstd -xf",
                _ => "-xf",
            };
            let strip = if depth > 0 {
                format!(" --strip-components={depth}")
            } else {
                String::new()
            };
            format!("tar {flag} {archive} -C {to}{strip}{members}")
        }
    };
    if spec.local && a.dst_local {
        cmd
    } else {
        format!("# {cmd}")
    }
}

fn connect_cmd(ep: &RemoteEndpoint) -> String {
    match ep.protocol {
        Protocol::S3 => format!("aws s3 ls{}", s3_endpoint_flag(ep)),
        // No shell client for the Drive API; open the web UI instead.
        Protocol::GoogleDrive => "open https://drive.google.com".to_string(),
        Protocol::Docker | Protocol::Kubernetes => format!("{} sh", ctr_exec_prefix(ep, true)),
        // No interactive shell client: open the server in the browser /
        // Finder instead.
        Protocol::WebDav | Protocol::WebDavs => {
            let scheme = if ep.protocol == Protocol::WebDavs {
                "https"
            } else {
                "http"
            };
            let port = if is_default_port(ep) {
                String::new()
            } else {
                format!(":{}", ep.port)
            };
            let host = if ep.host.contains(':') {
                format!("[{}]", ep.host)
            } else {
                ep.host.clone()
            };
            format!("open {scheme}://{host}{port}/")
        }
        Protocol::Smb => format!("open smb://{}", user_host(ep)),
        Protocol::Sftp | Protocol::Local => {
            let port = if is_default_port(ep) {
                String::new()
            } else {
                format!(" -P {}", ep.port)
            };
            format!("sftp{port} {}", user_host(ep))
        }
        Protocol::Ftp | Protocol::Ftps => {
            let host = if ep.host.contains(':') {
                format!("[{}]", ep.host)
            } else {
                ep.host.clone()
            };
            if is_default_port(ep) {
                format!("ftp {host}")
            } else {
                format!("ftp {host} {}", ep.port)
            }
        }
    }
}

fn sync_cmd(
    from: &VfsPath,
    src: &Side,
    to: &VfsPath,
    dst: &Side,
    spec: SyncSpec,
    ctx: ShellCtx,
) -> String {
    let ssh_e = [src.remote, dst.remote]
        .iter()
        .flatten()
        .find(|ep| !is_default_port(ep))
        .map(|ep| format!(" -e {}", quote(&format!("ssh -p {}", ep.port), ctx.kind)))
        .unwrap_or_default();
    let (_, a) = src.arg(from, true, ctx);
    let (_, b) = dst.arg(to, true, ctx);
    let a = dot_slash(a);
    let b = dot_slash(b);
    if spec.both_ways {
        format!("rsync -a -u{ssh_e} {a} {b} && rsync -a -u{ssh_e} {b} {a}")
    } else {
        let del = if spec.delete_extra { " --delete" } else { "" };
        format!("rsync -a{del}{ssh_e} {a} {b}")
    }
}

/// rsync copies the *contents* of a folder only with a trailing slash.
fn dot_slash(s: String) -> String {
    if s == "." || s.ends_with("..") {
        s + "/"
    } else {
        s
    }
}

// ------------------------------------------------------------------ results

/// How a finished action ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Failed,
    Cancelled,
}

/// "… (+3 more)" for list entries not shown.
fn more_note(n: usize) -> String {
    crate::i18n::tr!("term.more", n = n)
}

/// "12.4 MB" (decimal units, like Finder; "12,4 MB" in German).
pub fn format_bytes(n: u64) -> String {
    const UNITS: usize = 5;
    if n < 1000 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1000.0;
    let mut i = 0;
    while v >= 999.95 && i < UNITS - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{} {}", i18n::format_decimal(v, 1), i18n::size_unit(i))
}

/// "0.4 s", "12 ms", "2m 05s" (German: "0,4 s", "2 min 05 s").
pub fn format_elapsed(ms: u64) -> String {
    if ms < 100 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{} s", i18n::format_decimal(ms as f64 / 1000.0, 1))
    } else {
        let s = ms / 1000;
        let (m, s) = (s / 60, format!("{:02}", s % 60));
        tr!("term.result.minutes", m = m, s = s)
    }
}

/// One line, no control characters, at most `max` characters.
fn clean(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        let mut t: String = flat.chars().take(max - 1).collect();
        t.push('…');
        t
    } else {
        flat
    }
}

/// A short name for an action, used when its result is not printed right
/// below its command: `copy “a.txt” +2`.
pub fn short_label(a: &FileAction) -> String {
    let verb = match a.kind {
        FileActionKind::Copy => tr!("term.verb.copy"),
        FileActionKind::Move => tr!("term.verb.move"),
        FileActionKind::Delete => tr!("term.verb.delete"),
        FileActionKind::Trash => tr!("term.verb.trash"),
        FileActionKind::Rename => tr!("term.verb.rename"),
        FileActionKind::NewFolder => tr!("term.verb.mkdir"),
        FileActionKind::NewFile => tr!("term.verb.touch"),
        FileActionKind::Open => tr!("term.verb.open"),
        FileActionKind::Connect => tr!("term.verb.connect"),
        FileActionKind::Disconnect => tr!("term.verb.disconnect"),
        FileActionKind::Sync => tr!("term.verb.sync"),
        FileActionKind::Compress => tr!("term.verb.compress"),
        FileActionKind::Extract => tr!("term.verb.extract"),
    };
    let first = match a.kind {
        FileActionKind::Connect | FileActionKind::Disconnect => a
            .src_remote
            .as_ref()
            .map(user_host)
            .unwrap_or_else(|| a.src_label.clone()),
        _ => a
            .sources
            .first()
            .map(|p| {
                if p.is_root() {
                    "/".to_owned()
                } else {
                    p.file_name().to_owned()
                }
            })
            .unwrap_or_default(),
    };
    let more = a.sources.len().saturating_sub(1);
    let mut s = tr!("term.result.label", verb = verb, name = clean(&first, 60));
    if more > 0 && !matches!(a.kind, FileActionKind::Rename | FileActionKind::Sync) {
        s.push_str(&format!(" +{more}"));
    }
    s
}

/// The result line for a finished action, without the glyph: e.g.
/// `3 files · 12.4 MB · 0.4 s`, `failed: Permission denied (+2 more)`,
/// `cancelled`. `label` (see [`short_label`]) is put in front when the line
/// does not directly follow the command. `None` while the action runs.
pub fn result_text(a: &FileAction, label: Option<&str>) -> Option<(Outcome, String)> {
    let FileActionPhase::Finished {
        ok,
        files,
        bytes,
        errors,
        cancelled,
        elapsed_ms,
    } = &a.phase
    else {
        return None;
    };
    let (outcome, body) = if *cancelled {
        (Outcome::Cancelled, tr!("term.result.cancelled").to_owned())
    } else if !*ok {
        let body = match errors.first() {
            None => tr!("term.result.failed").to_owned(),
            Some(e) if errors.len() == 1 => {
                tr!("term.result.failed_error", error = clean(e, MAX_ERROR))
            }
            Some(e) => tr!(
                "term.result.failed_more",
                error = clean(e, MAX_ERROR),
                more = errors.len() - 1
            ),
        };
        (Outcome::Failed, body)
    } else {
        let mut parts = Vec::new();
        if *files > 0 {
            parts.push(match a.kind {
                FileActionKind::Delete | FileActionKind::Trash => {
                    tr!("term.result.items", count = *files)
                }
                _ => tr!("term.result.files", count = *files),
            });
        }
        if *bytes > 0 {
            parts.push(format_bytes(*bytes));
        }
        if parts.is_empty() {
            parts.push(tr!("term.result.done").into());
        }
        parts.push(format_elapsed(*elapsed_ms));
        (Outcome::Ok, parts.join(" · "))
    };
    let text = match label {
        Some(l) if outcome == Outcome::Ok => format!("{l} · {body}"),
        Some(l) => format!("{l} {body}"),
        None => body,
    };
    Some((outcome, text))
}

pub fn glyph(o: Outcome) -> &'static str {
    match o {
        Outcome::Ok => "✓",
        Outcome::Failed => "✗",
        Outcome::Cancelled => "⊝",
    }
}

// ------------------------------------------------------------------ styling

/// Styling of echoed lines. Uses the 16 ANSI colours so the terminal theme
/// (light or dark) picks readable shades; dim (SGR 2) for meta text.
pub mod style {
    pub const RESET: &str = "\x1b[0m";
    /// The arrow in front of a command.
    pub const ARROW: &str = "\x1b[0;1;36m";
    /// The command text.
    pub const COMMAND: &str = "\x1b[0;36m";
    /// Meta text (result details, notes).
    pub const META: &str = "\x1b[0;2m";
    pub const OK: &str = "\x1b[0;1;32m";
    pub const FAILED: &str = "\x1b[0;1;31m";
    pub const CANCELLED: &str = "\x1b[0;1;33m";
}

/// `→ cp -R 'a' 'b/'` with colours.
pub fn styled_command(cmd: &str) -> String {
    format!(
        "{}→{} {}{}{}",
        style::ARROW,
        style::RESET,
        style::COMMAND,
        cmd,
        style::RESET
    )
}

/// `  ✓ 3 files · 12.4 MB · 0.4 s` with colours.
pub fn styled_result(outcome: Outcome, text: &str) -> String {
    let color = match outcome {
        Outcome::Ok => style::OK,
        Outcome::Failed => style::FAILED,
        Outcome::Cancelled => style::CANCELLED,
    };
    // Failures keep the message readable (not dimmed).
    let body = if outcome == Outcome::Failed {
        style::RESET
    } else {
        style::META
    };
    format!(
        "  {color}{}{} {body}{text}{}",
        glyph(outcome),
        style::RESET,
        style::RESET
    )
}

/// A dim note line, e.g. about dropped lines.
pub fn styled_note(text: &str) -> String {
    format!("  {}{text}{}", style::META, style::RESET)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: ShellKind = ShellKind::Zsh;

    fn p(s: &str) -> VfsPath {
        VfsPath::new(s)
    }

    fn act(kind: FileActionKind, sources: &[&str], dest: Option<&str>) -> FileAction {
        let mut a = FileAction::new(
            1,
            manta_plugin::PaneId::Left,
            kind,
            sources.iter().map(|s| p(s)).collect(),
        );
        a.destination = dest.map(p);
        a
    }

    fn sftp(user: &str, host: &str, port: u16) -> RemoteEndpoint {
        RemoteEndpoint {
            context: String::new(),
            protocol: Protocol::Sftp,
            user: user.into(),
            host: host.into(),
            port,
        }
    }

    fn s3(host: &str, port: u16) -> RemoteEndpoint {
        RemoteEndpoint {
            context: String::new(),
            protocol: Protocol::S3,
            user: "AKIA".into(),
            host: host.into(),
            port,
        }
    }

    #[test]
    fn s3_commands() {
        let td = tempfile::tempdir().unwrap();
        let cwd = td.path().to_str().unwrap().to_owned();
        std::fs::create_dir(td.path().join("photos")).unwrap();
        let aws = s3("s3.eu-west-1.amazonaws.com", 443);
        // Upload a file and a folder.
        let mut a = act(
            FileActionKind::Copy,
            &[&format!("{cwd}/a.txt"), &format!("{cwd}/photos")],
            Some("/bkt/in"),
        );
        a.dst_local = false;
        a.dst_remote = Some(aws.clone());
        assert_eq!(
            cmd_in(&a, &cwd),
            "aws s3 cp 'a.txt' s3://bkt/in/ && aws s3 cp --recursive 'photos' s3://bkt/in/photos/"
        );
        // Download with a custom endpoint (MinIO on localhost).
        let mut a = act(FileActionKind::Move, &["/bkt/My File.pdf"], Some(&cwd));
        a.src_local = false;
        a.src_remote = Some(s3("localhost", 9000));
        assert_eq!(
            cmd_in(&a, &cwd),
            "aws s3 mv --endpoint-url http://localhost:9000 's3://bkt/My File.pdf' ."
        );
        // Delete a folder, new bucket / folder, rename, connect.
        let mut a = act(FileActionKind::Delete, &["/bkt/logs"], None);
        a.src_local = false;
        a.src_remote = Some(aws.clone());
        assert_eq!(cmd_in(&a, &cwd), "aws s3 rm --recursive s3://bkt/logs");
        a.kind = FileActionKind::NewFolder;
        assert_eq!(
            cmd_in(&a, &cwd),
            "aws s3api put-object --bucket bkt --key 'logs/'"
        );
        a.sources = vec![p("/newbucket")];
        assert_eq!(cmd_in(&a, &cwd), "aws s3 mb s3://newbucket");
        a.kind = FileActionKind::Rename;
        a.sources = vec![p("/bkt/a.txt")];
        a.destination = Some(p("/bkt/b.txt"));
        assert_eq!(cmd_in(&a, &cwd), "aws s3 mv s3://bkt/a.txt s3://bkt/b.txt");
        a.kind = FileActionKind::Connect;
        a.sources.clear();
        a.src_remote = Some(s3("fsn1.your-objectstorage.com", 443));
        assert_eq!(
            cmd_in(&a, &cwd),
            "aws s3 ls --endpoint-url https://fsn1.your-objectstorage.com"
        );
        // S3 <-> SFTP is not one command.
        let mut a = act(FileActionKind::Copy, &["/bkt/x.bin"], Some("/srv"));
        a.src_local = false;
        a.dst_local = false;
        a.src_remote = Some(aws);
        a.dst_remote = Some(sftp("u", "h", 22));
        assert!(cmd_in(&a, &cwd).starts_with("# aws s3 cp"));
    }

    #[test]
    fn container_commands() {
        let td = tempfile::tempdir().unwrap();
        let cwd = td.path().to_str().unwrap().to_owned();
        let web = RemoteEndpoint {
            context: String::new(),
            protocol: Protocol::Docker,
            user: String::new(),
            host: "web".into(),
            port: 0,
        };
        let pod = RemoteEndpoint {
            context: String::new(),
            protocol: Protocol::Kubernetes,
            user: "app".into(),
            host: "shop/web-0".into(),
            port: 0,
        };
        // Upload into a container, download from a pod.
        let mut a = act(
            FileActionKind::Copy,
            &[&format!("{cwd}/a b.txt")],
            Some("/srv/www"),
        );
        a.dst_local = false;
        a.dst_remote = Some(web.clone());
        assert_eq!(cmd_in(&a, &cwd), "docker cp 'a b.txt' web:/srv/www/");
        let mut a = act(FileActionKind::Move, &["/srv/log.txt"], Some(&cwd));
        a.src_local = false;
        a.src_remote = Some(pod.clone());
        assert_eq!(
            cmd_in(&a, &cwd),
            "kubectl cp -c app shop/web-0:/srv/log.txt . && kubectl exec -n shop web-0 -c app -- rm -r '/srv/log.txt'"
        );
        // Delete, mkdir, rename, connect inside a container.
        let mut a = act(FileActionKind::Delete, &["/srv/it's.txt"], None);
        a.src_local = false;
        a.src_remote = Some(web.clone());
        assert_eq!(
            cmd_in(&a, &cwd),
            r"docker exec web rm -r '/srv/it'\''s.txt'"
        );
        a.kind = FileActionKind::NewFolder;
        a.sources = vec![p("/srv/new")];
        assert_eq!(cmd_in(&a, &cwd), "docker exec web mkdir '/srv/new'");
        a.kind = FileActionKind::Rename;
        a.sources = vec![p("/srv/a")];
        a.destination = Some(p("/srv/b"));
        assert_eq!(cmd_in(&a, &cwd), "docker exec web mv '/srv/a' '/srv/b'");
        a.kind = FileActionKind::Connect;
        assert_eq!(cmd_in(&a, &cwd), "docker exec -it web sh");
        a.src_remote = Some(pod.clone());
        assert_eq!(
            cmd_in(&a, &cwd),
            "kubectl exec -it -n shop web-0 -c app -- sh"
        );
        // Container <-> SFTP: commented out.
        let mut a = act(FileActionKind::Copy, &["/srv/x"], Some("/home"));
        a.src_local = false;
        a.dst_local = false;
        a.src_remote = Some(web);
        a.dst_remote = Some(sftp("u", "h", 22));
        assert!(cmd_in(&a, &cwd).starts_with("# docker cp web:/srv/x"));
    }

    fn cmd_in(a: &FileAction, cwd: &str) -> String {
        let d = Dir::same(cwd);
        command(a, ShellCtx::new(Some(&d), Z))
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative_to("/a/b/c", "/a/b").as_deref(), Some("c"));
        assert_eq!(relative_to("/a/b", "/a/b/").as_deref(), Some("."));
        assert_eq!(relative_to("/a/x", "/a/b").as_deref(), Some("../x"));
        assert_eq!(relative_to("/a", "/a/b/c").as_deref(), Some("../.."));
        assert_eq!(relative_to("/x/y", "/a/b/c"), None, "at most two levels up");
        assert_eq!(relative_to("/etc/hosts", "/").as_deref(), Some("etc/hosts"));
        assert_eq!(relative_to("/ab/c", "/a"), Some("../ab/c".into()));
        // Shorter wins: absolute stays when relative is longer.
        let d = Dir::same("/Users/anna/Projects/deep/er");
        assert_eq!(local_display("/tmp/x", Some(&d)), "/tmp/x");
        assert_eq!(
            local_display("/Users/anna/Projects/deep/er/f.txt", Some(&d)),
            "f.txt"
        );
        assert_eq!(
            local_display("/Users/anna/Projects/deep/f.txt", Some(&d)),
            "../f.txt"
        );
        assert_eq!(
            local_display("/Users/anna/Projects/deep/er/f.txt", None),
            "/Users/anna/Projects/deep/er/f.txt"
        );
    }

    #[test]
    fn canonical_cwd_matches_spelled_path() {
        let tmp = tempfile::tempdir().unwrap();
        let canon = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::write(canon.join("f"), "").unwrap();
        // Shell reports the canonical cwd, the pane spells it differently
        // (on macOS /var → /private/var).
        let d = Dir::new(canon.clone(), canon.clone());
        let spelled = tmp.path().join("f").to_string_lossy().into_owned();
        assert_eq!(local_display(&spelled, Some(&d)), "f");
    }

    #[test]
    fn copy_move_delete_local() {
        let a = act(
            FileActionKind::Copy,
            &["/work/a.txt", "/work/it's here"],
            Some("/work/Design"),
        );
        assert_eq!(
            cmd_in(&a, "/work"),
            "cp -R 'a.txt' 'it'\\''s here' 'Design/'"
        );
        assert_eq!(
            cmd_in(&a, "/work/Design"),
            "cp -R '../a.txt' '../it'\\''s here' ."
        );
        let a = act(FileActionKind::Move, &["/work/a.txt"], Some("/work"));
        assert_eq!(cmd_in(&a, "/work/sub"), "mv '../a.txt' ..");
        let a = act(FileActionKind::Delete, &["/work/a", "/work/b c"], None);
        assert_eq!(cmd_in(&a, "/work"), "rm -r 'a' 'b c'");
        // Leading dash gets `--`.
        let a = act(FileActionKind::Delete, &["/work/-rf"], None);
        assert_eq!(cmd_in(&a, "/work"), "rm -r -- '-rf'");
        // Control characters use ANSI-C quoting.
        let a = act(FileActionKind::Delete, &["/work/a\nb"], None);
        assert_eq!(cmd_in(&a, "/work"), "rm -r $'a\\nb'");
    }

    #[test]
    fn trash_is_shown_as_a_move_to_the_trash() {
        let a = act(FileActionKind::Trash, &["/work/a", "/work/b c"], None);
        assert_eq!(cmd_in(&a, "/work"), "mv 'a' 'b c' ~/.Trash/");
        let a = act(FileActionKind::Trash, &["/work/-x"], None);
        assert_eq!(cmd_in(&a, "/work"), "mv -- '-x' ~/.Trash/");
        assert!(short_label(&a).contains("-x"));
    }

    #[test]
    fn batch_rename_lists_each_mv() {
        let mut a = act(
            FileActionKind::Rename,
            &["/work/a", "/work/b"],
            Some("/work/x"),
        );
        a.renames = vec![(p("/work/a"), p("/work/x")), (p("/work/b"), p("/work/y z"))];
        assert_eq!(cmd_in(&a, "/work"), "mv 'a' 'x'; mv 'b' 'y z'");
        a.renames = (1..=5)
            .map(|i| (p(&format!("/work/f{i}")), p(&format!("/work/g{i}"))))
            .collect();
        assert_eq!(
            cmd_in(&a, "/work"),
            "mv 'f1' 'g1'; mv 'f2' 'g2'; mv 'f3' 'g3' … (+2 more)"
        );
    }

    #[test]
    fn truncates_long_lists() {
        let srcs: Vec<String> = (1..=15).map(|i| format!("/work/f{i}")).collect();
        let refs: Vec<&str> = srcs.iter().map(String::as_str).collect();
        let a = act(FileActionKind::Copy, &refs, Some("/x"));
        assert_eq!(
            cmd_in(&a, "/work"),
            "cp -R 'f1' 'f2' 'f3' … (+12 more) '/x/'"
        );
        let a = act(FileActionKind::Delete, &refs[..4], None);
        assert_eq!(cmd_in(&a, "/work"), "rm -r 'f1' 'f2' 'f3' … (+1 more)");
        let a = act(FileActionKind::Delete, &refs[..3], None);
        assert_eq!(cmd_in(&a, "/work"), "rm -r 'f1' 'f2' 'f3'");
    }

    #[test]
    fn rename_mkdir_open() {
        let a = act(
            FileActionKind::Rename,
            &["/work/old name"],
            Some("/work/new"),
        );
        assert_eq!(cmd_in(&a, "/work"), "mv 'old name' 'new'");
        let a = act(FileActionKind::NewFolder, &["/work/New Folder"], None);
        assert_eq!(cmd_in(&a, "/work"), "mkdir 'New Folder'");
        assert_eq!(cmd_in(&a, "/elsewhere/deep/er"), "mkdir '/work/New Folder'");
        let a = act(FileActionKind::Open, &["/work/report.pdf"], None);
        assert_eq!(cmd_in(&a, "/work"), "open 'report.pdf'");
    }

    #[test]
    fn remote_commands() {
        // Upload.
        let mut a = act(FileActionKind::Copy, &["/work/a.txt"], Some("/srv/www"));
        a.dst_local = false;
        a.dst_remote = Some(sftp("anna", "example.org", 22));
        assert_eq!(
            cmd_in(&a, "/work"),
            "scp -r 'a.txt' anna@example.org:'/srv/www/'"
        );
        // Download on another port, as a move.
        let mut a = act(FileActionKind::Move, &["/srv/a b"], Some("/work"));
        a.kind = FileActionKind::Move;
        a.src_local = false;
        a.src_remote = Some(sftp("anna", "example.org", 2222));
        assert_eq!(
            cmd_in(&a, "/work"),
            "scp -r -P 2222 anna@example.org:'/srv/a b' . && ssh -p 2222 anna@example.org \"rm -r '/srv/a b'\""
        );
        // Server to server.
        let mut a = act(FileActionKind::Copy, &["/a"], Some("/b"));
        a.src_local = false;
        a.dst_local = false;
        a.src_remote = Some(sftp("x", "h1", 22));
        a.dst_remote = Some(sftp("", "h2", 22));
        assert_eq!(cmd_in(&a, "/work"), "scp -r -3 x@h1:'/a' h2:'/b/'");
        // Remote delete / rename / mkdir go through ssh.
        let mut a = act(FileActionKind::Delete, &["/srv/$x"], None);
        a.src_local = false;
        a.src_remote = Some(sftp("anna", "h", 22));
        assert_eq!(cmd_in(&a, "/work"), "ssh anna@h 'rm -r '\\''/srv/$x'\\'''");
        let mut a = act(FileActionKind::Rename, &["/srv/a"], Some("/srv/b"));
        a.src_local = false;
        a.src_remote = Some(sftp("anna", "h", 22));
        assert_eq!(cmd_in(&a, "/work"), "ssh anna@h \"mv '/srv/a' '/srv/b'\"");
        let mut a = act(FileActionKind::Open, &["/srv/r.pdf"], None);
        a.src_local = false;
        a.src_remote = Some(sftp("anna", "h", 22));
        assert_eq!(
            cmd_in(&a, "/work"),
            "scp anna@h:'/srv/r.pdf' /tmp/ && open '/tmp/r.pdf'"
        );
    }

    #[test]
    fn connect_and_disconnect() {
        let mut a = act(FileActionKind::Connect, &["/home/anna"], None);
        a.src_local = false;
        a.src_remote = Some(sftp("anna", "example.org", 22));
        assert_eq!(cmd_in(&a, "/work"), "sftp anna@example.org");
        a.src_remote = Some(sftp("anna", "example.org", 2222));
        assert_eq!(cmd_in(&a, "/work"), "sftp -P 2222 anna@example.org");
        a.src_remote = Some(RemoteEndpoint {
            context: String::new(),
            protocol: Protocol::Ftp,
            user: "u".into(),
            host: "ftp.x".into(),
            port: 21,
        });
        assert_eq!(cmd_in(&a, "/work"), "ftp ftp.x");
        a.src_remote = Some(RemoteEndpoint {
            context: String::new(),
            protocol: Protocol::Ftps,
            user: "u".into(),
            host: "ftp.x".into(),
            port: 990,
        });
        assert_eq!(cmd_in(&a, "/work"), "ftp ftp.x 990");
        a.kind = FileActionKind::Disconnect;
        assert_eq!(cmd_in(&a, "/work"), "# disconnect u@ftp.x (FTPS)");
    }

    #[test]
    fn sync_commands() {
        let mut a = act(FileActionKind::Sync, &["/work/site"], Some("/work/backup"));
        a.sync = Some(SyncSpec {
            delete_extra: true,
            both_ways: false,
        });
        assert_eq!(cmd_in(&a, "/work"), "rsync -a --delete 'site/' 'backup/'");
        a.sync = Some(SyncSpec {
            delete_extra: false,
            both_ways: true,
        });
        assert_eq!(
            cmd_in(&a, "/work/site"),
            "rsync -a -u ./ '../backup/' && rsync -a -u '../backup/' ./"
        );
        a.sync = None;
        a.dst_local = false;
        a.dst_remote = Some(sftp("anna", "h", 2200));
        assert_eq!(
            cmd_in(&a, "/work"),
            "rsync -a -e 'ssh -p 2200' 'site/' anna@h:'/work/backup/'"
        );
    }

    #[test]
    fn result_lines() {
        let fin = |kind, ok, files, bytes, errors: &[&str], cancelled, ms| {
            let mut a = act(kind, &["/work/a.txt", "/work/b", "/work/c"], Some("/x"));
            a.phase = FileActionPhase::Finished {
                ok,
                files,
                bytes,
                errors: errors.iter().map(|s| s.to_string()).collect(),
                cancelled,
                elapsed_ms: ms,
            };
            a
        };
        let a = fin(FileActionKind::Copy, true, 3, 12_400_000, &[], false, 400);
        assert_eq!(
            result_text(&a, None),
            Some((Outcome::Ok, "3 files · 12.4 MB · 0.4 s".into()))
        );
        assert_eq!(
            result_text(&a, Some(&short_label(&a))).unwrap().1,
            "copy “a.txt” +2 · 3 files · 12.4 MB · 0.4 s"
        );
        let a = fin(FileActionKind::Delete, true, 1, 0, &[], false, 12);
        assert_eq!(result_text(&a, None).unwrap().1, "1 item · 12 ms");
        let a = fin(FileActionKind::NewFolder, true, 0, 0, &[], false, 3);
        assert_eq!(result_text(&a, None).unwrap().1, "done · 3 ms");
        let a = fin(
            FileActionKind::Move,
            false,
            3,
            0,
            &["Permission denied:\n/x/a", "b", "c"],
            false,
            900,
        );
        assert_eq!(
            result_text(&a, None),
            Some((
                Outcome::Failed,
                "failed: Permission denied: /x/a (+2 more)".into()
            ))
        );
        let a = fin(FileActionKind::Move, false, 0, 0, &[], false, 900);
        assert_eq!(result_text(&a, None).unwrap().1, "failed");
        assert_eq!(
            result_text(&a, Some("move “a.txt” +2")).unwrap().1,
            "move “a.txt” +2 failed"
        );
        let a = fin(FileActionKind::Copy, false, 1, 10, &[], true, 900);
        assert_eq!(
            result_text(&a, None),
            Some((Outcome::Cancelled, "cancelled".into()))
        );
        let a = act(FileActionKind::Copy, &["/a"], Some("/b"));
        assert_eq!(result_text(&a, None), None, "still running");
        // Long errors are cut.
        let long = "x".repeat(500);
        let a = fin(FileActionKind::Copy, false, 0, 0, &[&long], false, 1);
        assert!(result_text(&a, None).unwrap().1.chars().count() < 200);
    }

    #[test]
    fn result_lines_in_german() {
        use manta_plugin::i18n::{with_lang, Lang};
        with_lang(Lang::De, || {
            let mut a = act(
                FileActionKind::Copy,
                &["/work/a.txt", "/work/b", "/work/c"],
                Some("/x"),
            );
            let finish = |a: &mut FileAction, ok, files, bytes, errors: &[&str], cancelled, ms| {
                a.phase = FileActionPhase::Finished {
                    ok,
                    files,
                    bytes,
                    errors: errors.iter().map(|s| s.to_string()).collect(),
                    cancelled,
                    elapsed_ms: ms,
                };
            };
            finish(&mut a, true, 3, 12_400_000, &[], false, 400);
            assert_eq!(
                result_text(&a, None).unwrap().1,
                "3 Dateien · 12,4 MB · 0,4 s"
            );
            assert!(styled_result(Outcome::Ok, &result_text(&a, None).unwrap().1).contains("✓"));
            assert_eq!(
                result_text(&a, Some(&short_label(&a))).unwrap().1,
                "kopieren „a.txt“ +2 · 3 Dateien · 12,4 MB · 0,4 s"
            );
            finish(&mut a, true, 1, 0, &[], false, 125_000);
            assert_eq!(result_text(&a, None).unwrap().1, "1 Datei · 2 min 05 s");
            a.kind = FileActionKind::Delete;
            finish(&mut a, true, 2, 0, &[], false, 12);
            assert_eq!(result_text(&a, None).unwrap().1, "2 Objekte · 12 ms");
            finish(&mut a, false, 0, 0, &["Zugriff verweigert", "b"], false, 1);
            assert_eq!(
                result_text(&a, None).unwrap().1,
                "fehlgeschlagen: Zugriff verweigert (+1 weitere)"
            );
            finish(&mut a, false, 0, 0, &[], true, 1);
            assert_eq!(result_text(&a, None).unwrap().1, "abgebrochen");
            assert_eq!(format_bytes(1_500_000_000), "1,5 GB");
            assert_eq!(
                crate::session::exit_note(),
                "[Prozess beendet – zum Neustarten den Zeilenschalter drücken]"
            );
        });
    }

    #[test]
    fn units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1000), "1.0 KB");
        assert_eq!(format_bytes(12_400_000), "12.4 MB");
        assert_eq!(format_bytes(999_999_999), "1.0 GB");
        assert_eq!(format_elapsed(99), "99 ms");
        assert_eq!(format_elapsed(400), "0.4 s");
        assert_eq!(format_elapsed(12_340), "12.3 s");
        assert_eq!(format_elapsed(125_000), "2m 05s");
    }

    #[test]
    fn styled_lines_reset_and_contain_no_newlines() {
        let c = styled_command("cp -R 'a' 'b/'");
        assert!(c.contains("→") && c.ends_with(style::RESET) && !c.contains('\n'));
        let r = styled_result(Outcome::Ok, "3 files");
        assert!(r.contains("✓") && r.ends_with(style::RESET));
        assert!(styled_result(Outcome::Failed, "x").contains("✗"));
        assert!(styled_result(Outcome::Cancelled, "x").contains("⊝"));
    }

    fn archive(path: &str, format: &str) -> Box<manta_plugin::ArchiveSpec> {
        Box::new(manta_plugin::ArchiveSpec {
            path: p(path),
            format: format.into(),
            local: true,
            remote: None,
            level: Some(6),
            exclude_junk: true,
        })
    }

    #[test]
    fn compress_commands() {
        let mut a = act(
            FileActionKind::Compress,
            &["/w/site", "/w/notes.txt"],
            Some("/w"),
        );
        a.archive = Some(archive("/w/site.zip", "zip"));
        assert_eq!(
            cmd_in(&a, "/w"),
            "zip -r 'site.zip' 'site' 'notes.txt' -x '*.DS_Store' '*__MACOSX*'"
        );
        assert_eq!(
            cmd_in(&a, "/elsewhere"),
            "(cd '/w' && zip -r '/w/site.zip' 'site' 'notes.txt' -x '*.DS_Store' '*__MACOSX*')"
        );
        let mut spec = archive("/w/site.tar.gz", "tar.gz");
        spec.exclude_junk = false;
        spec.level = Some(9);
        a.archive = Some(spec);
        a.sources.truncate(1);
        assert_eq!(cmd_in(&a, "/w"), "tar -czf 'site.tar.gz' 'site'");
        a.archive = Some(archive("/w/site.tar.zst", "tar.zst"));
        assert_eq!(
            cmd_in(&a, "/w/sub"),
            "tar --exclude .DS_Store --exclude __MACOSX --zstd -cf '/w/site.tar.zst' -C '/w' 'site'"
        );
        a.archive = Some(archive("/w/site.tar.xz", "tar.xz"));
        assert!(cmd_in(&a, "/w").contains("-cJf 'site.tar.xz'"));
        a.src_local = false;
        assert!(
            cmd_in(&a, "/w").starts_with("# "),
            "remote sources: not literal"
        );
        assert_eq!(short_label(&a), "compress “site”");
    }

    #[test]
    fn extract_commands() {
        let mut a = act(
            FileActionKind::Extract,
            &["/docs/a.txt", "/docs/b.txt"],
            Some("/out"),
        );
        a.archive = Some(archive("/dl/backup.zip", "zip"));
        assert_eq!(
            cmd_in(&a, "/dl"),
            "unzip -j 'backup.zip' 'docs/a.txt' 'docs/b.txt' -d '/out/'"
        );
        a.archive = Some(archive("/dl/backup.tar.gz", "tar.gz"));
        assert_eq!(
            cmd_in(&a, "/dl"),
            "tar -xzf 'backup.tar.gz' -C '/out/' --strip-components=1 'docs/a.txt' 'docs/b.txt'"
        );
        // The whole archive.
        a.sources.clear();
        a.archive = Some(archive("/dl/backup.tar.zst", "tar.zst"));
        assert_eq!(
            cmd_in(&a, "/dl"),
            "tar --zstd -xf 'backup.tar.zst' -C '/out/'"
        );
        a.archive = Some(archive("/dl/backup.zip", "zip"));
        assert_eq!(cmd_in(&a, "/dl"), "unzip 'backup.zip' -d '/out/'");
        let mut g = act(FileActionKind::Extract, &["/notes.txt"], Some("/dl"));
        g.archive = Some(archive("/dl/notes.txt.gz", "gzip"));
        assert_eq!(cmd_in(&g, "/dl"), "gunzip -c 'notes.txt.gz' > 'notes.txt'");
        let mut spec = archive("/dl/x.zip", "zip");
        spec.local = false;
        g.archive = Some(spec);
        assert!(cmd_in(&g, "/dl").starts_with("# unzip"));
        assert_eq!(short_label(&g), "extract “notes.txt”");
    }
}
