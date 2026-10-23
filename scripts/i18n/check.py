#!/usr/bin/env python3
"""Quick translation checks without cargo (the Rust tests check the same
and more):

    scripts/i18n/check.py              all languages, all catalogs
    scripts/i18n/check.py fr           one language
    scripts/i18n/check.py fr app/      one language, catalogs starting with "app/"
    scripts/i18n/check.py --stats      coverage per language

Reports missing and unknown keys, TODO markers, placeholder mismatches,
copies of English (plural forms equal to English always count, also the
forms the language never picks, like Chinese `.one` or Russian `.other`)
and punctuation that doesn't fit the language.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
import catalog as c  # noqa: E402

# Keep in sync with manta_plugin::i18n::check::SAME_EVERYWHERE.
SAME_EVERYWHERE = {
    "OK", "Manta", "Git", "SFTP", "FTP", "FTPS", "S3", "SSH", "WebDAV", "Docker", "Kubernetes",
    "Google Drive", "URL", "JSON", "ID", "PID", "API", "Markdown", "Readme", "README", "zip", "tar",
    "Access Key ID", "Secret Access Key", "Session Token", "Debug", "Trace", "PNG", "JPEG", "HEIC",
    "WebP", "TIFF", "GIF", "PDF", "SVG", "EXIF", "GPS", "Pod", "Pods", "Token", "Bucket", "Endpoint",
    "Shell", "Commit", "Branch", "Compose", "Namespace", "Cluster", "Beta",
}


def same_in():
    """Per-language exceptions, read from manta_plugin::i18n::check::SAME_IN."""
    src = open(os.path.join(c.ROOT, "crates/plugin/src/i18n.rs"), encoding="utf-8").read()
    block = src[src.index("pub const SAME_IN:"):]
    block = block[: block.index("];") + 2]
    out = {}
    names = {"Zh": "zh", "Es": "es", "Fr": "fr", "It": "it", "Ru": "ru"}
    for m in re.finditer(r"Lang::(\w+),\s*&\[(.*?)\]", block, re.S):
        out[names[m.group(1)]] = {c.unescape(x) for x in re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(2))}
    return out


SAME_IN = same_in()


def letters(s):
    return any(ch.isalpha() for ch in re.sub(r"\{[^{}]*\}", "", s))


def style(lang, en, s):
    out = []
    if "..." in s:
        out.append('use "…" not "..."')
    if "„" in s:
        out.append("German quotes „“")
    if lang == "fr" and ("“" in s or "”" in s) and ("“" in en):
        out.append("French quotes are « … »")
    if lang == "ru" and ("“" in s or "”" in s) and ("“" in en):
        out.append("Russian quotes are «…»")
    return out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    stats = "--stats" in sys.argv
    langs = [a for a in args if a in c.LANGS] or c.LANGS
    prefix = next((a for a in args if a not in c.LANGS), "")
    problems = 0
    total = {l: [0, 0] for l in langs}
    for name, src, paths in c.catalogs():
        if not name.startswith(prefix):
            continue
        source = c.read_source(src)
        for l in langs:
            need = c.needed_keys(source, l)
            path = paths[l]
            text = open(path, encoding="utf-8").read() if os.path.exists(path) else ""
            tr = c.read_translation(path)
            have = {}
            for k, s in tr:
                if k in have:
                    print(f"{name} [{l}] {k}: duplicate")
                    problems += 1
                have[k] = s
            todo_keys = set(re.findall(r'\("([^"]+)",.*// TODO', text))
            done = 0
            for k, (en, de, req, allowed) in need.items():
                s = have.get(k)
                msg = []
                if s is None:
                    msg.append("missing")
                else:
                    if k in todo_keys:
                        msg.append("TODO")
                    p = c.placeholders(s)
                    if not (req <= p <= allowed):
                        msg.append(f"placeholders {sorted(p)} vs {sorted(req)}")
                    same = en in SAME_EVERYWHERE or en in SAME_IN.get(l, ())
                    if s == en and letters(en) and not same and k not in todo_keys:
                        if c.is_plural_form(source, k):
                            # Even where German keeps the English: a plural
                            # form left in English is a missed translation.
                            msg.append("plural form is English")
                        elif de != en:
                            msg.append("copy of English (ok if intended: add to the catalog's same list)")
                    if not s.strip() and en.strip():
                        msg.append("empty")
                    msg += style(l, en, s)
                if not msg or msg == ["copy of English (ok if intended: add to the catalog's same list)"]:
                    done += 1
                if msg:
                    problems += 1
                    if not stats:
                        print(f"{name} [{l}] {k}: {'; '.join(msg)}\n    en: {en!r}\n    tr: {s!r}")
            for k in have:
                if k not in need:
                    problems += 1
                    if not stats:
                        print(f"{name} [{l}] {k}: not in the source (or not a plural form of {l})")
            total[l][0] += done
            total[l][1] += len(need)
    if stats:
        for l, (d, n) in total.items():
            print(f"{c.CODES[l]:8} {d:5}/{n:5}  {100.0 * d / max(n, 1):5.1f} %")
    else:
        print(f"{problems} problems")
    sys.exit(1 if problems and not stats else 0)


if __name__ == "__main__":
    main()
