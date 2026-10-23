#!/usr/bin/env python3
"""Generate docs/third-party-licenses.md: the licence notices of everything
linked into the `manta` binary (macOS, arm64 + x86_64).

    scripts/third-party-licenses.py            # (re)write the file
    scripts/third-party-licenses.py --check    # exit 1 if it is stale

Crates: `cargo tree` with normal edges only (no dev/build dependencies, no
proc macros, which don't end up in the binary), our own workspace crates
left out. For each crate the licence expression from Cargo.toml and the
texts of its LICENSE* / COPYING* / NOTICE* files (and those of bundled C
sources one folder down in `*-sys` crates) from the cargo registry sources.
Identical texts are printed once, with every crate that ships them.

Also lists copyleft licences (GPL, LGPL, AGPL) that are not an alternative
in an OR expression; --check fails on them.
"""

import json
import os
import re
import subprocess
import sys
from collections import OrderedDict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "docs", "third-party-licenses.md")
TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"]
NOTICE_RE = re.compile(r"^(licen[cs]e|copying|notice|copyright|unlicense)([-_.].*)?$", re.I)
# Not linked into the binary but vendored in the repository.
EXTRA = [
    ("Phosphor Icons (icon font)", "MIT", "crates/app/assets/fonts/Phosphor-LICENSE-MIT.txt"),
]


def cargo(*args):
    return subprocess.run(["cargo", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def linked_packages():
    tree_args = ["tree", "-p", "manta", "-e", "normal,no-proc-macro", "--prefix", "none", "-f", "{p}"]
    for t in TARGETS:
        tree_args += ["--target", t]
    wanted = set()
    for line in cargo(*tree_args).splitlines():
        m = re.match(r"^(\S+) v(\S+)", line.strip())
        if m:
            wanted.add((m.group(1), m.group(2)))
    by_id = {}
    own = set()
    for t in TARGETS:
        meta = json.loads(cargo("metadata", "--format-version", "1", "--filter-platform", t))
        own.update(meta["workspace_members"])
        by_id.update({p["id"]: p for p in meta["packages"]})
    pkgs = [p for i, p in by_id.items() if (p["name"], p["version"]) in wanted and i not in own]
    return sorted(pkgs, key=lambda p: (p["name"].lower(), p["version"]))


def notice_files(pkg):
    root = os.path.dirname(pkg["manifest_path"])
    found = []
    dirs = [root]
    if pkg["name"].endswith("-sys") or pkg["name"].endswith("-src"):
        dirs += [os.path.join(root, d) for d in sorted(os.listdir(root)) if os.path.isdir(os.path.join(root, d))]
    for d in dirs:
        for f in sorted(os.listdir(d)):
            path = os.path.join(d, f)
            if os.path.isfile(path) and NOTICE_RE.match(f) and not f.endswith((".rs", ".toml", ".json", ".py", ".sh")):
                found.append(path)
    if pkg.get("license_file"):
        lf = os.path.join(root, pkg["license_file"])
        if os.path.isfile(lf) and lf not in found:
            found.append(lf)
    return [(os.path.relpath(p, root), p) for p in found]


def normalize(text):
    lines = [l.rstrip() for l in text.replace("\r\n", "\n").replace("\t", "    ").split("\n")]
    return "\n".join(lines).strip("\n")


KINDS = [
    ("Apache License", "Apache License 2.0"),
    ("Mozilla Public License", "Mozilla Public License 2.0"),
    ("UNICODE LICENSE", "Unicode License v3"),
    ("Community Data License Agreement", "CDLA-Permissive-2.0"),
    ("SIL OPEN FONT LICENSE", "SIL Open Font License 1.1"),
    ("Ubuntu Font Licence", "Ubuntu Font Licence 1.0"),
    ("unencumbered software released into the public domain", "The Unlicense"),
    ("Creative Commons", "CC0 1.0"),
    ("bzip2", "bzip2 Licence"),
    ("Permission to use, copy, modify, and/or distribute", "ISC / 0BSD Licence"),
    ("Permission to use, copy, modify, and distribute", "ISC Licence"),
    ("Redistribution and use in source and binary forms", "BSD Licence"),
    ("This software is provided 'as-is'", "zlib Licence"),
    ("Permission is hereby granted, free of charge", "MIT License"),
]


def kind(text):
    low = text.lower()
    for needle, name in KINDS:
        if needle.lower() in low:
            return name
    return "Licence notice"


def is_gpl_text(text):
    head = text[:400].upper()
    return "GNU GENERAL PUBLIC LICENSE" in head or "GNU LESSER GENERAL PUBLIC LICENSE" in head


def copyleft(expr):
    """GPL-family licences that the expression requires (not just offers)."""
    if not expr:
        return []
    alternatives = re.split(r"\s+OR\s+|/", expr.replace("(", " ").replace(")", " "))
    if any(not re.search(r"\b(A|L)?GPL", a) for a in alternatives):
        return []
    return sorted(set(re.findall(r"\b(?:A|L)?GPL[-\w.+]*", expr)))


def fence(text):
    ticks = "```"
    while ticks in text:
        ticks += "`"
    return f"{ticks}text\n{text}\n{ticks}"


def render(pkgs):
    texts = OrderedDict()  # whitespace-insensitive key -> (text, crates)
    missing = []
    skipped_gpl = []
    for p in pkgs:
        files = notice_files(p)
        if not files:
            missing.append(p)
        for rel, path in files:
            with open(path, encoding="utf-8", errors="replace") as fh:
                t = normalize(fh.read())
            if is_gpl_text(t) and not copyleft(p["license"]):
                # Offered as an alternative only (self_cell, zstd's C
                # sources): Manta uses the permissive option.
                skipped_gpl.append(f"{p['name']} {p['version']}")
                continue
            if t:
                key = " ".join(t.split())
                if kind(t) == "Apache License 2.0" and "END OF TERMS AND CONDITIONS" in t and len(t) < 12000:
                    # One copy of the licence is what it asks for; the
                    # copies differ only in the appendix's example line.
                    key = "apache-2.0"
                if key not in texts:
                    texts[key] = (t, [])
                texts[key][1].append(f"{p['name']} {p['version']}" + ("" if rel.count("/") == 0 and rel.lower().startswith(("licen", "copying", "unlicense")) else f" ({rel})"))
    out = []
    w = out.append
    w("# Third-Party Licences")
    w("")
    w("Manta is free and open source under the MIT licence (see LICENSE). It is built on "
      "open-source components whose licences ask that their notices travel with every copy. "
      "This page lists them all: every crate linked into the app, its licence, and the licence texts as "
      "the authors ship them. Many thanks to everyone who wrote and maintains them.")
    w("")
    w("Generated by `scripts/third-party-licenses.py` from `Cargo.lock`; do not edit by hand.")
    w("")
    w("## Components")
    w("")
    w("| Component | Version | Licence |")
    w("|---|---|---|")
    for p in pkgs:
        lic = (p["license"] or "see licence file").replace("|", "\\|")
        w(f"| {p['name']} | {p['version']} | {lic} |")
    for name, lic, _ in EXTRA:
        w(f"| {name} | | {lic} |")
    w("")
    if missing:
        w("These crates ship no licence file of their own; the standard text of their licence is included "
          "below with other crates under the same licence:")
        w("")
        w(", ".join(f"{p['name']} {p['version']} ({p['license']})" for p in missing))
        w("")
    if skipped_gpl:
        w("Some components offer the GNU GPL as one of several licences ("
          + ", ".join(sorted(set(skipped_gpl)))
          + "). Manta uses them under their permissive alternative, so the GPL texts are not reproduced.")
        w("")
    w("## Licence texts")
    w("")
    for name, _, rel in EXTRA:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as fh:
            t = normalize(fh.read())
        w(f"### {name}")
        w("")
        w(fence(t))
        w("")
    for t, users in texts.values():
        w(f"### {kind(t)}")
        w("")
        w("Used by: " + ", ".join(users))
        w("")
        w(fence(t))
        w("")
    return "\n".join(out).rstrip("\n") + "\n"


def main():
    check = "--check" in sys.argv[1:]
    pkgs = linked_packages()
    flagged = [(p["name"], p["version"], p["license"]) for p in pkgs if copyleft(p["license"])]
    for n, v, l in flagged:
        print(f"copyleft licence: {n} {v} ({l})", file=sys.stderr)
    doc = render(pkgs)
    if check:
        try:
            with open(OUT, encoding="utf-8") as fh:
                current = fh.read()
        except FileNotFoundError:
            current = ""
        if current != doc:
            print(f"{os.path.relpath(OUT, ROOT)} is stale: run scripts/third-party-licenses.py", file=sys.stderr)
            return 1
        return 1 if flagged else 0
    with open(OUT, "w", encoding="utf-8") as fh:
        fh.write(doc)
    print(f"{os.path.relpath(OUT, ROOT)}: {len(pkgs)} crates, {len(doc) // 1024} KiB")
    return 1 if flagged else 0


if __name__ == "__main__":
    sys.exit(main())
