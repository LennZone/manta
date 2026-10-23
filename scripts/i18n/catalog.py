"""Read the string catalogs of the app and the built-in plugins.

Used by `scripts/i18n/check.py` (translation checks without cargo) and
`scripts/i18n/skeleton.py`. The Rust tests are the source of truth; this is
a quick helper for translators.

Source tables are `(key, English, German)` tuples, translation tables
`(key, text)` tuples, both plain Rust string literals.
"""

import os
import re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
LANGS = ["zh", "es", "fr", "it", "ru"]
CODES = {"zh": "zh-Hans", "es": "es", "fr": "fr", "it": "it", "ru": "ru"}
# CLDR plural forms (integers) a translation needs per plural entry.
# Chinese "one" and Russian "other" (fractions) are never picked for whole
# numbers but are filled anyway, so no form ever falls back to English.
PLURAL_FORMS = {"zh": ["one", "other"], "ru": ["one", "few", "many", "other"], "es": ["one", "other"], "fr": ["one", "other"], "it": ["one", "other"]}

_STR = r'"((?:[^"\\]|\\.)*)"'
_TUPLE3 = re.compile(r"\(\s*" + _STR + r"\s*,\s*" + _STR + r"\s*,\s*" + _STR + r"\s*,?\s*\)", re.S)
_TUPLE2 = re.compile(r"\(\s*" + _STR + r"\s*,\s*" + _STR + r"\s*,?\s*\)", re.S)


def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            if n == "n":
                out.append("\n"); i += 2
            elif n == "t":
                out.append("\t"); i += 2
            elif n in "\"\\'":
                out.append(n); i += 2
            elif n == "u":
                j = s.index("}", i)
                out.append(chr(int(s[i + 3:j], 16))); i = j + 1
            elif n == "\n":
                i += 2
                while i < len(s) and s[i] in " \t\n":
                    i += 1
            else:
                out.append(c); i += 1
        else:
            out.append(c); i += 1
    return "".join(out)


def escape(s):
    out = []
    for c in s:
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif c == "\n":
            out.append("\\n")
        elif c == "\t":
            out.append("\\t")
        elif c in "  ":
            out.append("\\u{%x}" % ord(c))
        else:
            out.append(c)
    return "".join(out)


def _array_body(text, const="ENTRIES"):
    m = re.search(r"const " + const + r"\s*:[^=]*=\s*&\[", text)
    if not m:
        return ""
    i, depth = m.end(), 1
    in_str = False
    j = i
    while j < len(text) and depth:
        c = text[j]
        if in_str:
            if c == "\\":
                j += 1
            elif c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "/" and text[j:j + 2] == "//":
            j = text.index("\n", j)
            continue
        elif c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
        j += 1
    return text[i:j - 1]


def _strip_comments(body):
    out, in_str, i = [], False, 0
    while i < len(body):
        c = body[i]
        if in_str:
            out.append(c)
            if c == "\\":
                out.append(body[i + 1]); i += 2; continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True; out.append(c)
        elif body[i:i + 2] == "//":
            i = body.find("\n", i)
            if i < 0:
                break
            continue
        else:
            out.append(c)
        i += 1
    return "".join(out)


def read_source(path):
    """[(key, en, de)] of a source table."""
    body = _strip_comments(_array_body(open(path, encoding="utf-8").read()))
    return [tuple(unescape(x) for x in m.groups()) for m in _TUPLE3.finditer(body)]


def read_translation(path):
    """[(key, text)] of a translation table (empty if the file is missing)."""
    if not os.path.exists(path):
        return []
    body = _strip_comments(_array_body(open(path, encoding="utf-8").read()))
    return [tuple(unescape(x) for x in m.groups()) for m in _TUPLE2.finditer(body)]


def catalogs():
    """[(name, source_path, {lang: translation_path})] for the app areas and the plugins."""
    out = []
    cat = os.path.join(ROOT, "crates/app/src/i18n/catalog")
    for f in sorted(os.listdir(cat)):
        if f.endswith(".rs") and f != "mod.rs":
            area = f[:-3]
            out.append(("app/" + area, os.path.join(cat, f), {l: os.path.join(cat, l, f) for l in LANGS}))
    crates = os.path.join(ROOT, "crates")
    for d in sorted(os.listdir(crates)):
        src = os.path.join(crates, d, "src/i18n.rs")
        if d.startswith("plugin-") and os.path.exists(src):
            out.append((d, src, {l: os.path.join(crates, d, "src/i18n", l + ".rs") for l in LANGS}))
    return out


def placeholders(s):
    return set(re.findall(r"\{([^{}]*)\}", s))


def is_plural_form(source, key):
    """Is `key` a form (`.one`, `.few`, `.many`, `.other`) of a plural entry?"""
    keys = {k for k, _, _ in source}
    base, _, form = key.rpartition(".")
    return form in ("one", "few", "many", "other") and base + ".one" in keys and base + ".other" in keys


def needed_keys(source, lang):
    """{key: (en, de, required_placeholders, allowed_placeholders)} a translation needs."""
    keys = {k: (e, d) for k, e, d in source}
    need = {}
    for k, e, d in source:
        base = None
        for suf in (".one", ".other"):
            if k.endswith(suf):
                b = k[: -len(suf)]
                if b + ".one" in keys and b + ".other" in keys:
                    base = b
        if base is None:
            need[k] = (e, d, placeholders(e), placeholders(e))
            continue
        one, other = keys[base + ".one"], keys[base + ".other"]
        allowed = placeholders(one[0]) | placeholders(other[0])
        for f in PLURAL_FORMS[lang]:
            only_1 = f == "one" and lang not in ("fr", "ru")
            src = one if only_1 else other
            need.setdefault(base + "." + f, (src[0], src[1], placeholders(src[0]), allowed))
    return need
