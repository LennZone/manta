#!/usr/bin/env python3
"""Package a Manta plugin folder as a .mantaplugin file.

    python3 scripts/plugin-pack.py PLUGIN_DIR                   writes ./<id>-<version>.mantaplugin
    python3 scripts/plugin-pack.py PLUGIN_DIR -o dist/          … into dist/
    python3 scripts/plugin-pack.py PLUGIN_DIR -o my.mantaplugin … with this name
    python3 scripts/plugin-pack.py PLUGIN_DIR --url https://github.com/me/p/releases/download/v1.2.0/p-1.2.0.mantaplugin

What it does:

1. Checks PLUGIN_DIR/plugin.toml the way Manta does (scripts/plugin-test.py
   --manifest-only); errors stop it.
2. Zips the folder into one top-level folder named after the plugin id,
   leaving out .git, __pycache__, *.pyc, .DS_Store, target/ and
   .pytest_cache. Unix permissions (the executable bit) are kept; times
   are fixed, so the same files always give the same archive and SHA-256.
   Symbolic links are refused (Manta doesn't install them).
3. Prints the file, its size and its SHA-256, and the "latest" block for
   plugins/registry.json (with --url: the download URL of the release).

A .mantaplugin file is a zip archive; users install it by double-clicking
it, dropping it on Settings > Plugins or with Install Plugin > From a File.
Exit code 0: written; 1: problems; 2: bad usage. Python 3.9+ standard
library only.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import stat
import sys
import zipfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SKIP_DIRS = {".git", "__pycache__", ".pytest_cache", "target", ".idea", ".vscode"}
SKIP_FILES = {".DS_Store"}
# Manta's unpacking limits (crates/plugin/src/install.rs).
MAX_ENTRIES = 10_000
MAX_BYTES = 512 * 1024 * 1024
FIXED_TIME = (1980, 1, 1, 0, 0, 0)


def load_checker():
    path = os.path.join(ROOT, "scripts", "plugin-test.py")
    spec = importlib.util.spec_from_file_location("plugin_test", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def files_of(src):
    """(relative path, absolute path) of every file to pack, sorted. Raises
    ValueError on symbolic links."""
    out = []
    for dirpath, dirnames, filenames in os.walk(src, followlinks=False):
        for d in list(dirnames):
            full = os.path.join(dirpath, d)
            if os.path.islink(full):
                raise ValueError(f"symbolic link: {os.path.relpath(full, src)} (Manta doesn't install links)")
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for f in sorted(filenames):
            if f in SKIP_FILES or f.endswith(".pyc") or f.endswith(".mantaplugin"):
                continue
            full = os.path.join(dirpath, f)
            if os.path.islink(full):
                raise ValueError(f"symbolic link: {os.path.relpath(full, src)} (Manta doesn't install links)")
            out.append((os.path.relpath(full, src).replace(os.sep, "/"), full))
    return out


def pack(src, dest, top):
    """Write the archive. Returns (entries, unpacked bytes)."""
    entries = files_of(src)
    if len(entries) > MAX_ENTRIES:
        raise ValueError(f"{len(entries)} files; Manta installs at most {MAX_ENTRIES}")
    total = sum(os.path.getsize(f) for _, f in entries)
    if total > MAX_BYTES:
        raise ValueError(f"{total >> 20} MB; Manta installs at most {MAX_BYTES >> 20} MB")
    tmp = dest + ".tmp"
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        folder = zipfile.ZipInfo(top + "/", FIXED_TIME)
        folder.external_attr = (stat.S_IFDIR | 0o755) << 16 | 0x10
        z.writestr(folder, b"")
        for rel, full in entries:
            info = zipfile.ZipInfo(f"{top}/{rel}", FIXED_TIME)
            mode = os.stat(full).st_mode & 0o777
            # Only rwx bits; readable by the owner at least.
            info.external_attr = (stat.S_IFREG | mode | 0o600) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3  # Unix: keep the permissions
            with open(full, "rb") as f:
                z.writestr(info, f.read())
    os.replace(tmp, dest)
    return len(entries), total


def main():
    ap = argparse.ArgumentParser(description="Package a Manta plugin folder as a .mantaplugin file.")
    ap.add_argument("plugin", help="the plugin folder (with plugin.toml)")
    ap.add_argument("-o", "--output", help="output file or folder (default: the current folder)")
    ap.add_argument("--url", help="the release's download URL, for the registry snippet")
    args = ap.parse_args()
    src = os.path.abspath(args.plugin)
    if not os.path.isdir(src):
        print(f"not a folder: {args.plugin}")
        return 2

    checker = load_checker()
    report = checker.Report(False)
    print(f"checking     {src}/plugin.toml")
    manifest = checker.check_manifest(src, report)
    if manifest is None or report.errors:
        print(f"FAILED: plugin.toml has {len(set(report.errors))} error(s); nothing was written")
        return 1

    pid, version = manifest["id"], str(manifest["version"])
    name = f"{pid}-{version}.mantaplugin"
    out = args.output or "."
    # A name ending in .mantaplugin / .zip is the file; anything else a folder.
    is_file = out.lower().endswith((".mantaplugin", ".zip")) and not os.path.isdir(out)
    dest = out if is_file else os.path.join(out, name)
    dest = os.path.abspath(dest)
    if dest.startswith(src + os.sep):
        print("the output must not be inside the plugin folder")
        return 2
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    try:
        count, total = pack(src, dest, pid)
    except (ValueError, OSError) as e:
        print(f"FAILED: {e}")
        return 1

    digest = hashlib.sha256(open(dest, "rb").read()).hexdigest()
    size = os.path.getsize(dest)
    print(f"wrote        {dest}")
    print(f"contents     {count} file(s), {total:,} bytes unpacked; archive {size:,} bytes")
    print(f"sha256       {digest}")
    latest = {
        "version": version,
        "url": args.url or f"https://github.com/<you>/<repo>/releases/download/v{version}/{name}",
        "sha256": digest,
    }
    print('\n"latest" for plugins/registry.json:')
    print(json.dumps({"latest": latest}, indent=2, ensure_ascii=False)[2:-2])
    return 0


if __name__ == "__main__":
    sys.exit(main())
