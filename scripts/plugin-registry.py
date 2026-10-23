#!/usr/bin/env python3
"""Check the community plugin directory (plugins/registry.json).

    python3 scripts/plugin-registry.py                 check the format (offline)
    python3 scripts/plugin-registry.py --fetch         also download every release, verify its
                                                       SHA-256 and compare its plugin.toml
    python3 scripts/plugin-registry.py --fetch --only my-plugin
    python3 scripts/plugin-registry.py --registry other.json

The format is described in plugins/registry.json's schema
(plugins/registry.schema.json) and docs/plugins.md, "Publishing a plugin".
With --fetch, each release archive is unpacked (safely, in a temporary
folder) and its plugin.toml is checked with scripts/plugin-test.py
--manifest-only. Exit code 1 on problems. Python 3.9+ standard library only.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
REGISTRY = os.path.join(ROOT, "plugins", "registry.json")
LANGS = {"en", "de", "zh-Hans", "es", "fr", "it", "ru"}
PERMISSIONS = {"read_files", "write_files", "network", "run_programs", "terminal"}
PLATFORMS = {"any", "macos-arm64"}
RUNTIMES = {"python3", "node", "shell", "native", "other"}
# OSI-approved licences seen in practice (SPDX ids). Extend when needed:
# https://opensource.org/licenses
OSI = {
    "MIT", "MIT-0", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "0BSD", "ISC", "Zlib", "Unlicense",
    "MPL-2.0", "LGPL-2.1-only", "LGPL-2.1-or-later", "LGPL-3.0-only", "LGPL-3.0-or-later",
    "GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0-only", "GPL-3.0-or-later", "AGPL-3.0-only",
    "AGPL-3.0-or-later", "EPL-2.0", "EUPL-1.2", "Artistic-2.0", "BSL-1.0", "UPL-1.0", "CDDL-1.0",
}
ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{1,63}$")
FIELDS = {"id", "name", "description", "author", "repository", "homepage", "license", "latest",
          "min_manta_version", "platforms", "runtime", "permissions", "tags"}
REQUIRED = {"id", "name", "description", "author", "repository", "license", "latest",
            "min_manta_version", "platforms"}


def license_ok(expr):
    """An SPDX expression whose ids are all OSI-approved (`MIT OR Apache-2.0`)."""
    ids = [t for t in re.split(r"\s+(?:OR|AND|WITH)\s+|[()]", expr) if t.strip()]
    return bool(ids) and all(t.strip() in OSI for t in ids)


def check(data):
    problems = []
    if not isinstance(data, dict) or data.get("schema") != 1 or not isinstance(data.get("plugins"), list):
        return ['the file must be {"schema": 1, "plugins": [...]}']
    extra = set(data) - {"schema", "plugins"}
    if extra:
        problems.append(f"unknown top-level keys {sorted(extra)}")
    seen = set()
    for n, p in enumerate(data["plugins"]):
        where = f"plugins[{n}]"
        if not isinstance(p, dict):
            problems.append(f"{where}: must be an object")
            continue
        pid = p.get("id")
        where = f"{where} ({pid})"
        for k in sorted(REQUIRED - set(p)):
            problems.append(f"{where}: \"{k}\" is missing")
        for k in sorted(set(p) - FIELDS):
            problems.append(f"{where}: unknown key \"{k}\"")
        if not isinstance(pid, str) or not ID_RE.match(pid):
            problems.append(f"{where}: id must be 2–64 lowercase letters, digits, '.', '-', '_'")
        elif pid == "manta" or pid.startswith("manta.") or pid == "rustlift" or pid.startswith("rustlift."):
            problems.append(f"{where}: the id is reserved for built-in plugins")
        if pid in seen:
            problems.append(f"{where}: id used twice")
        seen.add(pid)
        for key in ("name", "description"):
            t = p.get(key)
            if not isinstance(t, dict) or not isinstance(t.get("en"), str) or not t["en"].strip():
                problems.append(f"{where}: \"{key}\" needs an English text: {{\"en\": \"…\"}}")
                continue
            for lang, text in t.items():
                if lang not in LANGS:
                    problems.append(f"{where}: {key}: unknown language \"{lang}\" (use {sorted(LANGS)})")
                if not isinstance(text, str) or not text.strip() or len(text) > 300:
                    problems.append(f"{where}: {key}.{lang} must be 1–300 characters")
        if not isinstance(p.get("author"), str) or not p.get("author", "").strip():
            problems.append(f"{where}: \"author\" must not be empty")
        for key in ("repository", "homepage"):
            if key in p and not (isinstance(p[key], str) and p[key].startswith("https://")):
                problems.append(f"{where}: \"{key}\" must be an https:// URL")
        lic = p.get("license")
        if not isinstance(lic, str) or not license_ok(lic):
            problems.append(f"{where}: license {lic!r} isn't an OSI-approved SPDX id (see OSI in this script)")
        latest = p.get("latest")
        if not isinstance(latest, dict):
            problems.append(f"{where}: \"latest\" must be {{version, url, sha256}}")
        else:
            for k in sorted({"version", "url", "sha256"} - set(latest)):
                problems.append(f"{where}: latest.{k} is missing")
            if not (isinstance(latest.get("url"), str) and latest["url"].startswith("https://")):
                problems.append(f"{where}: latest.url must be an https:// URL")
            if not (isinstance(latest.get("sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", latest["sha256"])):
                problems.append(f"{where}: latest.sha256 must be 64 lowercase hex digits")
            if not isinstance(latest.get("version"), str) or not latest.get("version", "").strip():
                problems.append(f"{where}: latest.version must not be empty")
        mmv = p.get("min_manta_version")
        if not (isinstance(mmv, str) and re.fullmatch(r"\d+\.\d+(\.\d+)?", mmv)):
            problems.append(f"{where}: min_manta_version must look like \"0.3.0\"")
        plats = p.get("platforms")
        if not (isinstance(plats, list) and plats and all(x in PLATFORMS for x in plats)):
            problems.append(f"{where}: platforms must be a non-empty list of {sorted(PLATFORMS)}")
        if "runtime" in p and p["runtime"] not in RUNTIMES:
            problems.append(f"{where}: runtime must be one of {sorted(RUNTIMES)}")
        perms = p.get("permissions", [])
        if not (isinstance(perms, list) and all(x in PERMISSIONS for x in perms)):
            problems.append(f"{where}: permissions must be a list of {sorted(PERMISSIONS)}")
        tags = p.get("tags", [])
        if not (isinstance(tags, list) and len(tags) <= 6 and all(isinstance(t, str) and re.fullmatch(r"[a-z0-9-]{1,24}", t) for t in tags)):
            problems.append(f"{where}: tags: at most 6, lowercase letters, digits and '-'")
    return problems


def fetch(p):
    """Download, verify and inspect one release. Returns problems."""
    pid, latest = p["id"], p["latest"]
    out = []
    with tempfile.TemporaryDirectory(prefix="manta-registry-") as tmp:
        path = os.path.join(tmp, "release.zip")
        try:
            req = urllib.request.Request(latest["url"], headers={"User-Agent": "manta-plugin-registry-check"})
            with urllib.request.urlopen(req, timeout=60) as r, open(path, "wb") as f:
                f.write(r.read(600 * 1024 * 1024))
        except Exception as e:  # noqa: BLE001
            return [f"{pid}: can't download {latest['url']}: {e}"]
        digest = hashlib.sha256(open(path, "rb").read()).hexdigest()
        if digest != latest["sha256"]:
            out.append(f"{pid}: SHA-256 mismatch: registry {latest['sha256']}, file {digest}")
        unpacked = os.path.join(tmp, "unpacked")
        try:
            with zipfile.ZipFile(path) as z:
                for info in z.infolist():
                    name = info.filename
                    if name.startswith("/") or ".." in name.split("/"):
                        return out + [f"{pid}: unsafe path in the archive: {name}"]
                    if (info.external_attr >> 16) & 0o170000 == 0o120000:
                        return out + [f"{pid}: symbolic link in the archive: {name}"]
                z.extractall(unpacked)
        except zipfile.BadZipFile as e:
            return out + [f"{pid}: not a zip archive: {e}"]
        root = unpacked
        if not os.path.isfile(os.path.join(root, "plugin.toml")):
            subs = [d for d in os.listdir(unpacked) if not d.startswith(".") and d != "__MACOSX"]
            if len(subs) == 1 and os.path.isfile(os.path.join(unpacked, subs[0], "plugin.toml")):
                root = os.path.join(unpacked, subs[0])
            else:
                return out + [f"{pid}: no plugin.toml at the top of the archive or in one folder"]
        test = subprocess.run(
            [sys.executable, os.path.join(ROOT, "scripts", "plugin-test.py"), root, "--manifest-only"],
            capture_output=True, text=True, env=dict(os.environ, NO_COLOR="1"))
        if test.returncode != 0:
            out.append(f"{pid}: plugin.toml fails scripts/plugin-test.py:\n{test.stdout}")
        sys.path.insert(0, os.path.join(ROOT, "scripts"))
        import importlib.util
        spec = importlib.util.spec_from_file_location("plugin_test", os.path.join(ROOT, "scripts", "plugin-test.py"))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        m = mod.load_toml(open(os.path.join(root, "plugin.toml"), encoding="utf-8").read())
        for key, want in (("id", pid), ("version", latest["version"]), ("license", p.get("license")),
                          ("min_manta_version", p.get("min_manta_version"))):
            if m.get(key) != want:
                out.append(f"{pid}: plugin.toml {key} = {m.get(key)!r}, registry says {want!r}")
        if sorted(m.get("permissions", [])) != sorted(p.get("permissions", [])):
            out.append(f"{pid}: permissions differ: plugin.toml {m.get('permissions', [])}, registry {p.get('permissions', [])}")
    return out


def main():
    ap = argparse.ArgumentParser(description="Check plugins/registry.json.")
    ap.add_argument("--registry", default=REGISTRY)
    ap.add_argument("--fetch", action="store_true", help="download and verify every release")
    ap.add_argument("--only", help="with --fetch: only this id")
    args = ap.parse_args()
    try:
        data = json.load(open(args.registry, encoding="utf-8"))
    except (OSError, ValueError) as e:
        print(f"{args.registry}: {e}")
        return 1
    problems = check(data)
    if not problems and args.fetch:
        for p in data["plugins"]:
            if args.only and p["id"] != args.only:
                continue
            print(f"fetching {p['id']} {p['latest']['version']} …")
            problems += fetch(p)
    for pr in problems:
        print("problem: " + pr)
    n = len(data.get("plugins", [])) if isinstance(data, dict) else 0
    print(f"{args.registry}: {n} plugin(s), {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
