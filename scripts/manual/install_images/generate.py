#!/usr/bin/env python3
"""Draw the install illustrations of the manual (chapter 01 and 19, the website's download
section) in every manual language:

    python3 scripts/manual/install_images/generate.py                 # all images, all languages
    python3 scripts/manual/install_images/generate.py --lang de,ru    # some languages
    python3 scripts/manual/install_images/generate.py --only gatekeeper-blocked
    python3 scripts/manual/install_images/generate.py --keep-html DIR # also keep the HTML pages

Writes docs/manual/images/<lang>/{gatekeeper-blocked,gatekeeper-open-anyway,gatekeeper-confirm}.png
and docs/manual/images/en/install-dmg.png (no macOS text in it, so one copy serves every
language).

These show macOS UI (the disk image window, the Gatekeeper dialogs, System Settings), not
Manta's, so they are illustrations, not screenshots: HTML/CSS pages styled like current macOS
(light appearance, system font, PingFang SC for Chinese), rendered at 2x by headless Google
Chrome with a throwaway profile. Nothing is installed or launched and no system setting is
read or changed, and the pictures exist in every language, not only the Mac's own. All macOS
text comes from strings.json (Apple's localized strings, see extract_strings.py); the app
icon and the disk image background come from packaging/. A coloured ring and a pointer mark
the button to click. Each page checks itself for clipped text before the screenshot; an
overflow fails the run. With Pillow installed the PNGs are reduced to 256 colours like the
other manual images (the app build embeds them as lossless WebP).
"""
import argparse
import html
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
IMAGES = ROOT / "docs" / "manual" / "images"
ICON = ROOT / "packaging" / "icon" / "AppIcon-1024.png"
DMG_BG = ROOT / "packaging" / "dmg" / "background@2x.png"
LANGS = ["de", "en", "zh-Hans", "es", "fr", "it", "ru"]
CHROME_CANDIDATES = [
    os.environ.get("CHROME", ""),
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
]
APP = "Manta"

S = json.loads((HERE / "strings.json").read_text(encoding="utf-8"))["strings"]


def t(name, lang):
    return html.escape(S[name][lang].replace("%@", APP))


# ------------------------------------------------------------------ shared CSS / pieces
CSS = """
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { width: var(--w); height: var(--h); }
body {
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", "PingFang SC", sans-serif;
  -webkit-font-smoothing: antialiased; color: #1d1d1f; overflow: hidden;
  background: #e9edf2;
}
:lang(zh-Hans) body, body:lang(zh-Hans) {
  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "Hiragino Sans GB", sans-serif;
}
.stage { position: absolute; left: 0; top: 0; width: var(--w); height: var(--h); display: grid; place-items: center; }
/* the marker: a warm ring around the control to click */
.ring { position: relative; }
.ring::after {
  content: ""; position: absolute; inset: -5px; border-radius: inherit;
  border: 3px solid #ff6b3d; box-shadow: 0 0 0 4px rgba(255,107,61,.22), 0 0 18px rgba(255,107,61,.35);
  pointer-events: none;
}
.cursor { position: absolute; width: 22px; height: 33px; filter: drop-shadow(0 2px 2px rgba(0,0,0,.3)); z-index: 10; }
/* Tahoe-style alert */
.alert {
  width: 300px; padding: 20px 20px 18px; border-radius: 26px; position: relative;
  background: rgba(248,248,250,.98);
  box-shadow: 0 0 0 .5px rgba(0,0,0,.16), 0 16px 36px rgba(20,30,60,.24), 0 3px 10px rgba(20,30,60,.10);
}
.alert .icon { width: 64px; height: 64px; display: block; }
.alert .help {
  position: absolute; top: 16px; right: 16px; width: 22px; height: 22px; border-radius: 50%;
  background: #fff; box-shadow: 0 0 0 .5px rgba(0,0,0,.18), 0 1px 2px rgba(0,0,0,.12);
  font: 600 13px/22px -apple-system, sans-serif; text-align: center; color: #555;
}
.alert h1 { font-size: 13px; font-weight: 700; line-height: 1.3; margin: 14px 0 6px; }
.alert p { font-size: 11px; line-height: 1.4; color: #333; }
.alert .buttons { display: flex; flex-direction: column; gap: 9px; margin-top: 18px; }
.btn span { display: block; overflow: hidden; }
.btn {
  height: 28px; border-radius: 14px; font-size: 13px; line-height: 28px; text-align: center;
  padding: 0 14px; white-space: nowrap;
  background: rgba(0,0,0,.075); color: #1d1d1f;
}
.btn.default { background: linear-gradient(#2f8cff, #0a6cf0); color: #fff; font-weight: 500; }
/* window chrome */
.window {
  position: relative; background: #fff; border-radius: 16px; overflow: visible;
  box-shadow: 0 0 0 .5px rgba(0,0,0,.2), 0 26px 70px rgba(20,30,60,.30), 0 6px 18px rgba(20,30,60,.12);
}
.lights { display: flex; gap: 8px; }
.lights i { width: 12px; height: 12px; border-radius: 50%; display: block; box-shadow: inset 0 0 0 .5px rgba(0,0,0,.18); }
.lights i:nth-child(1) { background: #ff5f57; } .lights i:nth-child(2) { background: #febc2e; } .lights i:nth-child(3) { background: #28c840; }
"""

CURSOR = ('<svg class="cursor" style="{style}" viewBox="0 0 22 33" aria-hidden="true">'
          '<path d="M2 2 L2 26 L8 20.5 L12.2 30.5 L16.4 28.7 L12.3 19 L20 19 Z" fill="#000" stroke="#fff" '
          'stroke-width="2" stroke-linejoin="round"/></svg>')

# The pointer rests on the lower right part of the ringed control, placed after layout.
PLACE_CURSOR = """<script>(function(){var r=document.querySelector('.ring').getBoundingClientRect(),
  c=document.querySelector('.cursor');c.style.left=(r.right-Math.min(34,r.width*.15))+'px';c.style.top=(r.bottom-10)+'px';})();</script>"""

# Overflow self-check: every [data-fit] box must contain its text, and every [data-inside]
# box must lie inside the viewport. The result lands in #report (read via --dump-dom).
CHECK_JS = """
<script>
(function(){
  var bad=[];
  document.querySelectorAll('[data-fit]').forEach(function(e){
    if (e.scrollWidth > e.clientWidth + 1 || e.scrollHeight > e.clientHeight + 1)
      bad.push('clipped:' + (e.getAttribute('data-fit') || e.className) + ':' + e.scrollWidth + 'x' + e.scrollHeight + '>' + e.clientWidth + 'x' + e.clientHeight);
  });
  document.querySelectorAll('[data-inside]').forEach(function(e){
    var r=e.getBoundingClientRect();
    var W=+document.body.dataset.w, H=+document.body.dataset.h;
    if (r.left < 0 || r.top < 0 || r.right > W + .5 || r.bottom > H + .5)
      bad.push('outside:' + e.getAttribute('data-inside') + ':' + Math.round(r.left) + ',' + Math.round(r.top) + '-' + Math.round(r.right) + 'x' + Math.round(r.bottom) + ' in ' + W + 'x' + H);
  });
  var d=document.createElement('div'); d.id='report'; d.hidden=true;
  d.textContent = bad.length ? bad.join('|') : 'ok'; document.body.appendChild(d);
})();
</script>
"""


def page(lang, w, h, body, extra_css=""):
    return (f'<!doctype html><html lang="{lang}"><head><meta charset="utf-8">'
            f'<meta name="viewport" content="width={w}, height={h}">'
            f"<style>:root {{ --w: {w}px; --h: {h}px; }}{CSS}{extra_css}</style></head>"
            f'<body data-w="{w}" data-h="{h}">{body}{CHECK_JS}</body></html>')


def icon_url():
    return ICON.as_uri()


# ------------------------------------------------------------------ scenes
def scene_install_dmg(lang):
    """The disk image window (packaging/dmg): drag Manta onto Applications."""
    w, h = 760, 528
    css = """
    .dmg { width: 660px; }
    .dmg .bar { height: 28px; display: flex; align-items: center; padding: 0 10px; position: relative;
                border-radius: 16px 16px 0 0; background: #f6f6f6; border-bottom: .5px solid rgba(0,0,0,.12); }
    .dmg .bar b { position: absolute; left: 0; right: 0; text-align: center; font-size: 13px; font-weight: 600; color: #3a3a3c; }
    .dmg .content { position: relative; width: 660px; height: 400px; border-radius: 0 0 16px 16px; overflow: hidden;
                    background: url('BG') 0 0 / 660px 400px no-repeat; }
    .item { position: absolute; width: 150px; margin-left: -75px; text-align: center; font-size: 13px; color: #1d1d1f; }
    .item img, .item svg { width: 128px; height: 128px; display: block; margin: 0 auto 4px; }
    .ghost { position: absolute; width: 96px; height: 96px; opacity: .5; }
    .drop { position: absolute; width: 160px; height: 172px; border-radius: 22px; }
    """.replace("BG", DMG_BG.as_uri())
    # Finder places the icon centre at the given point (dmgbuild_settings.py: 165,190 and 495,190)
    folder = """<svg viewBox="0 0 128 128" aria-hidden="true">
      <defs><linearGradient id="fb" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#6cc4f5"/><stop offset="1" stop-color="#3aa3e6"/></linearGradient>
      <linearGradient id="ff" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#8fd6fb"/><stop offset="1" stop-color="#4fb3ef"/></linearGradient></defs>
      <path d="M10 30 q0-8 8-8 h30 q5 0 8 4 l5 6 h49 q8 0 8 8 v62 q0 8-8 8 H18 q-8 0-8-8 z" fill="url(#fb)"/>
      <rect x="8" y="42" width="112" height="70" rx="8" fill="url(#ff)"/>
      <g fill="none" stroke="#1f7fc4" stroke-width="5" stroke-linecap="round" opacity=".75">
        <path d="M54 94 l10-30 l10 30 M58 84 h12"/>
        <path d="M46 98 h36"/>
      </g></svg>"""
    body = f"""
    <div class="stage"><div class="window dmg" data-inside="window">
      <div class="bar"><div class="lights"><i></i><i></i><i></i></div><b>{APP}</b></div>
      <div class="content">
        <div class="item" style="left:165px; top:126px"><img src="{icon_url()}" alt=""><span>{APP}</span></div>
        <div class="drop ring" style="left:415px; top:116px"></div>
        <div class="item" style="left:495px; top:126px">{folder}<span>Applications</span></div>
        <img class="ghost" src="{icon_url()}" alt="" style="left:318px; top:214px">
        {CURSOR.format(style="left:366px; top:262px")}
      </div>
    </div></div>"""
    return w, h, page(lang, w, h, body, css)


def alert(lang, icon, title, text, buttons, ring_index):
    btns = "".join(
        f'<div class="btn{" default" if d else ""}{" ring" if i == ring_index else ""}"><span data-fit="button {i}">{label}</span></div>'
        for i, (label, d) in enumerate(buttons))
    return f"""
    <div class="stage"><div class="alert" data-inside="alert">
      <img class="icon" src="{icon}" alt="">
      <div class="help">?</div>
      <h1>{title}</h1>
      <p>{text}</p>
      <div class="buttons">{btns}</div>
    </div></div>{CURSOR.format(style="left:0; top:0")}{PLACE_CURSOR}"""


def scene_gatekeeper_blocked(lang):
    """First launch: “Manta” Not Opened – click Done (not Move to Trash)."""
    w, h = 380, 400
    body = alert(lang, icon_url(), t("blocked_title", lang), t("blocked_text", lang),
                 [(t("done", lang), True), (t("move_to_trash", lang), False)], 0)
    return w, h, page(lang, w, h, body)


def scene_gatekeeper_confirm(lang):
    """After Open Anyway in System Settings: Open “Manta”? – click Open Anyway."""
    w, h = 380, 420
    body = alert(lang, icon_url(), t("confirm_title", lang), t("confirm_text", lang),
                 [(t("open_anyway_dialog", lang), False), (t("done_dialog", lang), True)], 0)
    return w, h, page(lang, w, h, body)


SIDEBAR_ICONS = {
    # name: (background, glyph svg path(s) in a 20x20 box, white)
    "sb_general": ("#8e8e93", '<circle cx="10" cy="10" r="5.2" fill="none" stroke="#fff" stroke-width="2.2" stroke-dasharray="2.4 1.6"/><circle cx="10" cy="10" r="2" fill="#fff"/>'),
    "sb_appearance": ("#1d1d1f", '<circle cx="10" cy="10" r="5.5" fill="none" stroke="#fff" stroke-width="1.6"/><path d="M10 4.5 a5.5 5.5 0 0 1 0 11 z" fill="#fff"/>'),
    "sb_accessibility": ("#0a7aff", '<circle cx="10" cy="5.3" r="1.6" fill="#fff"/><path d="M5 8 h10 M10 8 v4 l-2.6 4.5 M10 12 l2.6 4.5" stroke="#fff" stroke-width="1.7" fill="none" stroke-linecap="round"/>'),
    "sb_menu_bar": ("#0a7aff", '<rect x="4" y="5" width="12" height="10" rx="2" fill="none" stroke="#fff" stroke-width="1.5"/><path d="M4 8.2 h12" stroke="#fff" stroke-width="1.5"/>'),
    "sb_siri": ("#5e5ce6", '<path d="M4 10 q3-6 6 0 t6 0" stroke="#fff" stroke-width="1.8" fill="none"/>'),
    "sb_spotlight": ("#8e8e93", '<circle cx="9" cy="9" r="3.8" fill="none" stroke="#fff" stroke-width="1.8"/><path d="M11.8 11.8 l3.4 3.4" stroke="#fff" stroke-width="2" stroke-linecap="round"/>'),
    "sb_privacy_security": ("#0a7aff", '<path d="M8 15.5 c-2.5-1-3.5-3-3.5-5 v-3 c0-.8 1.2-.8 1.2 0 v2 v-4.3 c0-.8 1.2-.8 1.2 0 v4 v-5 c0-.8 1.2-.8 1.2 0 v5 v-4.4 c0-.8 1.2-.8 1.2 0 v5.2 l1.4-1.8 c.6-.7 1.6 0 1.1.8 l-2.2 3.6 c-.7 1.3-1.6 2.5-2.6 2.9 z" fill="#fff"/>'),
    "sb_desktop_dock": ("#1d1d1f", '<rect x="4" y="5" width="12" height="10" rx="1.6" fill="none" stroke="#fff" stroke-width="1.5"/><rect x="6.5" y="11.6" width="7" height="1.8" rx=".9" fill="#fff"/>'),
    "sb_displays": ("#0a7aff", '<circle cx="10" cy="10" r="2.6" fill="#fff"/><path d="M10 3.5 v2 M10 14.5 v2 M3.5 10 h2 M14.5 10 h2 M5.4 5.4 l1.4 1.4 M13.2 13.2 l1.4 1.4 M5.4 14.6 l1.4-1.4 M13.2 6.8 l1.4-1.4" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/>'),
    "sb_wallpaper": ("#30b0c7", '<circle cx="10" cy="10" r="1.8" fill="#fff"/><g fill="none" stroke="#fff" stroke-width="1.5"><ellipse cx="10" cy="6" rx="1.8" ry="2.6"/><ellipse cx="10" cy="14" rx="1.8" ry="2.6"/><ellipse cx="6" cy="10" rx="2.6" ry="1.8"/><ellipse cx="14" cy="10" rx="2.6" ry="1.8"/></g>'),
}


def scene_open_anyway(lang):
    """System Settings › Privacy & Security, scrolled to Security: Open Anyway."""
    w, h = 820, 560
    css = """
    .settings { width: 740px; height: 480px; display: flex; overflow: hidden; border-radius: 22px; background: #f2f2f4; }
    .side { width: 214px; margin: 8px; border-radius: 16px; padding: 10px 8px; position: relative;
            background: rgba(232,234,239,.95); box-shadow: 0 0 0 .5px rgba(0,0,0,.08); }
    .side .lights { margin: 4px 0 14px 6px; }
    .search { height: 28px; border-radius: 14px; background: rgba(0,0,0,.06); display: flex; align-items: center; gap: 6px;
              padding: 0 10px; color: #8a8a8e; font-size: 13px; margin-bottom: 12px; }
    .search svg { width: 13px; height: 13px; flex: none; }
    .sb { display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 4px 8px; border-radius: 8px; font-size: 13px; line-height: 1.2; }
    .sb .ic { width: 20px; height: 20px; border-radius: 5px; flex: none; display: grid; place-items: center; }
    .sb .ic svg { width: 20px; height: 20px; }
    .sb span { overflow: hidden; }
    .sb.sel { background: #0a6cf0; color: #fff; }
    .sb.sel .ic { box-shadow: 0 0 0 1px rgba(255,255,255,.5); }
    .gap { height: 10px; }
    .main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
    .head { height: 52px; display: flex; align-items: center; gap: 12px; padding: 0 18px 0 10px; }
    .nav { display: flex; height: 28px; border-radius: 14px; background: rgba(255,255,255,.85);
           box-shadow: 0 0 0 .5px rgba(0,0,0,.12); }
    .nav i { width: 30px; display: grid; place-items: center; color: #9a9a9e; font-style: normal; font-size: 15px; }
    .head b { font-size: 15px; font-weight: 700; white-space: nowrap; }
    .scroll { flex: 1; padding: 0 22px 0 18px; overflow: hidden; position: relative; }
    .scroll::before { content: ""; position: absolute; left: 0; right: 0; top: 0; height: 40px; z-index: 2;
                      background: linear-gradient(#f2f2f4, rgba(242,242,244,0)); }
    .group { background: #fff; border-radius: 12px; box-shadow: 0 0 0 .5px rgba(0,0,0,.09); margin-bottom: 18px; }
    .row { display: flex; align-items: center; gap: 14px; min-height: 40px; padding: 9px 12px; font-size: 13px; }
    .row + .row { border-top: .5px solid rgba(0,0,0,.09); }
    .row .lbl { flex: 1; min-width: 0; line-height: 1.35; }
    .row .chev { color: #b0b0b5; font-size: 14px; }
    .faded { opacity: .55; }
    h2 { font-size: 13px; font-weight: 700; margin: 4px 0 8px 4px; }
    .popup { display: flex; align-items: center; gap: 6px; font-size: 13px; color: #3a3a3c; max-width: 58%; text-align: right; }
    .popup svg { width: 9px; height: 14px; flex: none; }
    .pbtn { height: 24px; line-height: 24px; padding: 0 12px; border-radius: 12px; font-size: 13px; white-space: nowrap; flex: none;
            background: #fff; box-shadow: 0 0 0 .5px rgba(0,0,0,.2), 0 1px 2px rgba(0,0,0,.12); }
    .pbtn > span { display: block; overflow: hidden; }
    .pbtn.ring::after { inset: -6px; border-radius: 16px; }
    """
    items = ["sb_general", "sb_appearance", "sb_accessibility", "sb_menu_bar", "sb_siri", "sb_spotlight",
             "sb_privacy_security", None, "sb_desktop_dock", "sb_displays", "sb_wallpaper"]
    side = []
    for it in items:
        if it is None:
            side.append('<div class="gap"></div>')
            continue
        bg, glyph = SIDEBAR_ICONS[it]
        label = t("privacy_security" if it == "sb_privacy_security" else it, lang)
        sel = " sel" if it == "sb_privacy_security" else ""
        side.append(f'<div class="sb{sel}"><div class="ic" style="background:{bg}"><svg viewBox="0 0 20 20">{glyph}</svg></div>'
                    f'<span data-fit="sidebar {it}">{label}</span></div>')
    magnifier = '<svg viewBox="0 0 13 13"><circle cx="5.5" cy="5.5" r="4.2" fill="none" stroke="#8a8a8e" stroke-width="1.5"/><path d="M8.6 8.6 L12 12" stroke="#8a8a8e" stroke-width="1.6" stroke-linecap="round"/></svg>'
    updown = '<svg viewBox="0 0 9 14"><path d="M1.5 5 L4.5 2 L7.5 5 M1.5 9 L4.5 12 L7.5 9" fill="none" stroke="#3a3a3c" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>'
    body = f"""
    <div class="stage"><div class="window settings" data-inside="window">
      <div class="side">
        <div class="lights"><i></i><i></i><i></i></div>
        <div class="search">{magnifier}<span>{t("search", lang)}</span></div>
        {''.join(side)}
      </div>
      <div class="main">
        <div class="head"><div class="nav"><i>‹</i><i>›</i></div><b data-fit="title">{t("privacy_security", lang)}</b></div>
        <div class="scroll">
          <div class="group faded">
            <div class="row"><span class="lbl">{t("analytics", lang)}</span><span class="chev">›</span></div>
            <div class="row"><span class="lbl">{t("advertising", lang)}</span><span class="chev">›</span></div>
          </div>
          <h2>{t("security", lang)}</h2>
          <div class="group" data-inside="security">
            <div class="row"><span class="lbl">{t("allow_apps_from", lang)}</span>
              <span class="popup"><span>{t("app_store_known", lang)}</span>{updown}</span></div>
            <div class="row"><span class="lbl">{t("was_blocked", lang)}</span>
              <span class="pbtn ring"><span data-fit="open anyway">{t("open_anyway", lang)}</span></span></div>
          </div>
          <div class="group faded">
            <div class="row"><span class="lbl">{t("filevault", lang)}</span><span class="chev">›</span></div>
          </div>
        </div>
      </div>
    </div>
    <div id="cur"></div></div>"""
    body += CURSOR.format(style="left:0; top:0") + PLACE_CURSOR
    return w, h, page(lang, w, h, body, css)


# Images without macOS text exist once, in en/ (the help window and the web manual fall back
# to English for every language).
NEUTRAL = {"install-dmg"}

SCENES = {
    "install-dmg": scene_install_dmg,
    "gatekeeper-blocked": scene_gatekeeper_blocked,
    "gatekeeper-open-anyway": scene_open_anyway,
    "gatekeeper-confirm": scene_gatekeeper_confirm,
}


# ------------------------------------------------------------------ rendering
def chrome():
    for c in CHROME_CANDIDATES:
        if c and Path(c).exists():
            return c
    sys.exit("generate.py: Google Chrome not found (set CHROME=/path/to/chrome)")


def run_chrome(exe, profile, args):
    cmd = [exe, "--headless=new", "--no-first-run", "--no-default-browser-check", "--disable-extensions",
           "--disable-sync", "--disable-background-networking", "--hide-scrollbars", "--allow-file-access-from-files",
           f"--user-data-dir={profile}", "--force-device-scale-factor=2", "--force-color-profile=srgb"] + args
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        sys.exit(f"generate.py: chrome failed: {r.stderr.strip()[-500:]}")
    return r.stdout


def finish_png(path, w, h):
    try:
        from PIL import Image
    except ImportError:
        return
    with Image.open(path) as im:
        if im.size != (w * 2, h * 2):
            sys.exit(f"generate.py: {path} is {im.size}, expected {(w * 2, h * 2)}")
        # full colour: the gradients and shadows band at 256 colours
        rgb = im.convert("RGB")
    rgb.save(path, optimize=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--lang", default=",".join(LANGS))
    ap.add_argument("--only", default=",".join(SCENES))
    ap.add_argument("--keep-html", metavar="DIR")
    args = ap.parse_args()
    langs = [l for l in args.lang.split(",") if l]
    names = [n for n in args.only.split(",") if n]
    for l in langs:
        if l not in LANGS:
            sys.exit(f"generate.py: unknown language {l}")
    for n in names:
        if n not in SCENES:
            sys.exit(f"generate.py: unknown image {n}")
    exe = chrome()
    tmp = Path(tempfile.mkdtemp(prefix="manta-install-images-"))
    failed = []
    try:
        profile = tmp / "profile"
        for lang in langs:
            for name in names:
                if name in NEUTRAL and lang != "en":
                    stale = IMAGES / lang / f"{name}.png"
                    if stale.exists():
                        stale.unlink()
                    continue
                w, h, doc = SCENES[name](lang)
                src = tmp / f"{name}-{lang}.html"
                src.write_text(doc, encoding="utf-8")
                dom = run_chrome(exe, profile, [f"--window-size={w},{h}", "--dump-dom", src.as_uri()])
                report = dom.split('<div id="report" hidden="">', 1)[-1].split("</div>", 1)[0]
                if report != "ok":
                    failed.append(f"{lang}/{name}: {report}")
                out = IMAGES / lang / f"{name}.png"
                out.parent.mkdir(parents=True, exist_ok=True)
                run_chrome(exe, profile, [f"--window-size={w},{h}", f"--screenshot={out}", src.as_uri()])
                finish_png(out, w, h)
                print(f"  {out.relative_to(ROOT)}  {w * 2}×{h * 2}" + ("" if report == "ok" else "  ✗ " + report))
        if args.keep_html:
            dest = Path(args.keep_html)
            dest.mkdir(parents=True, exist_ok=True)
            for f in tmp.glob("*.html"):
                shutil.copy(f, dest / f.name)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    if failed:
        print("generate.py: text does not fit:\n  " + "\n  ".join(failed), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
