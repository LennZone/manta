#!/usr/bin/env python3
"""App icon tooling: the first-generation concepts (legacy) authored as SVG, rasterised with resvg.

    python3 packaging/icon/make_icons.py            # default concept (see DEFAULT)
    python3 packaging/icon/make_icons.py --icon b   # make concept b the app icon
    python3 packaging/icon/make_icons.py --set manta --icon a   # Manta set (packaging/icon/manta/)

Everything is generated from this file, so the icon is reproducible:

  packaging/icon/src/<c>.svg, <c>_small.svg   SVG sources (small = 16/32 px variant)
  packaging/icon/candidates/<c>_1024.png      each concept at 1024 px
  packaging/icon/candidates/sheet.png         all concepts at 1024..16 px, light and dark
  packaging/icon/AppIcon-1024.png             master of the chosen concept
  packaging/icon/AppIcon.icns                 for Manta.app/Contents/Resources
  crates/app/assets/app-icon.png              512 px window/Dock icon embedded in the binary

Concepts
  a  "Fork"    two glass panes, a copper lift fork carrying a document between them
  b  "R"       a white "R" built from two stacked panes; its leg is a copper arrow
  c  "Folder"  a blue Finder-style folder on a lift platform with copper cables

The body follows Apple's macOS grid: an 824 px continuous-corner squircle inside
1024 px with the standard soft drop shadow. Copper is only an accent; the main
colour is the app's accent blue (macOS system blue, `Accent::Blue` in theme.rs).

Requires: Pillow, cargo (builds packaging/icon/render, a tiny resvg wrapper, offline
from the local registry cache) and, for the .icns, macOS `iconutil`.
"""
import argparse
import math
import os
import shutil
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
SRC = os.path.join(HERE, "src")
CAND = os.path.join(HERE, "candidates")
RENDER_DIR = os.path.join(HERE, "render")

DEFAULT = "b"
CONCEPTS = {"a": "Fork", "b": "R", "c": "Folder"}

# ---------------------------------------------------------------- shared parts


def squircle(x0=100, y0=100, x1=924, y1=924, n=4.3, steps=360):
    """Superellipse path, the shape of Apple's continuous-corner icon body."""
    cx, cy, a, b = (x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2, (y1 - y0) / 2
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        pts.append((cx + a * math.copysign(abs(c) ** (2 / n), c),
                    cy + b * math.copysign(abs(s) ** (2 / n), s)))
    return "M" + " L".join(f"{x:.2f},{y:.2f}" for x, y in pts) + " Z"


SQ = squircle()

COMMON_DEFS = f"""
  <filter id="blur28" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="14"/></filter>
  <filter id="soft" x="-30%" y="-30%" width="160%" height="160%">
    <feGaussianBlur in="SourceAlpha" stdDeviation="9"/>
    <feOffset dy="8"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.28"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="lift" x="-30%" y="-30%" width="160%" height="160%">
    <feGaussianBlur in="SourceAlpha" stdDeviation="12"/>
    <feOffset dy="14"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.38"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <clipPath id="body"><path d="{SQ}"/></clipPath>
  <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#fff" stop-opacity="0.55"/>
    <stop offset="0.35" stop-color="#fff" stop-opacity="0.08"/>
    <stop offset="1" stop-color="#000" stop-opacity="0.18"/>
  </linearGradient>
  <radialGradient id="sheen" cx="512" cy="40" r="620" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#fff" stop-opacity="0.30"/>
    <stop offset="1" stop-color="#fff" stop-opacity="0"/>
  </radialGradient>
  <linearGradient id="copper" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#F7B27A"/>
    <stop offset="0.45" stop-color="#E07B3F"/>
    <stop offset="1" stop-color="#A9461E"/>
  </linearGradient>
  <linearGradient id="copperV" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#F5A86E"/>
    <stop offset="1" stop-color="#B4501F"/>
  </linearGradient>
  <linearGradient id="paper" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#FFFFFF"/>
    <stop offset="1" stop-color="#E9F0FA"/>
  </linearGradient>
"""


def body(fill_id):
    """Shadow, filled squircle, top sheen and inner rim light."""
    return f"""
  <path d="{SQ}" fill="#000" fill-opacity="0.34" filter="url(#blur28)" transform="translate(0 12)"/>
  <path d="{SQ}" fill="url(#{fill_id})"/>
  <path d="{SQ}" fill="url(#sheen)"/>
  <path d="{SQ}" fill="none" stroke="url(#rim)" stroke-width="6" clip-path="url(#body)"/>"""


def svg(defs, content):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs>{COMMON_DEFS}{defs}
</defs>{content}
</svg>
"""


def blue_bg(id_, top="#4FA9FF", bottom="#0059D6"):
    return f"""
  <linearGradient id="{id_}" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="{top}"/>
    <stop offset="1" stop-color="{bottom}"/>
  </linearGradient>"""


# -------------------------------------------------------------- concept a: Fork


def concept_a(small):
    defs = blue_bg("bg") + """
  <linearGradient id="glass" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#fff" stop-opacity="0.34"/>
    <stop offset="1" stop-color="#fff" stop-opacity="0.16"/>
  </linearGradient>"""
    out = [body("bg")]
    # Two frosted glass panes.
    if small:
        panes = [(176, 214, 470, 772), (554, 214, 848, 772)]
    else:
        panes = [(186, 226, 466, 746), (558, 226, 838, 746)]
    for i, (x0, y0, x1, y1) in enumerate(panes):
        out.append(f'<rect x="{x0}" y="{y0}" width="{x1-x0}" height="{y1-y0}" rx="44" '
                   f'fill="url(#glass)" stroke="#fff" stroke-opacity="0.55" stroke-width="{6 if small else 3}"/>')
        if not small:
            out.append(f'<rect x="{x0}" y="{y0}" width="{x1-x0}" height="64" rx="44" fill="#fff" fill-opacity="0.22"/>'
                       f'<rect x="{x0}" y="{y0+40}" width="{x1-x0}" height="24" fill="#fff" fill-opacity="0.22"/>')
            for dx in (0, 28, 56):
                out.append(f'<circle cx="{x0+38+dx}" cy="{y0+32}" r="9" fill="#fff" fill-opacity="0.75"/>')
            for j, w in enumerate((190, 140, 170, 120, 156)):
                y = y0 + 104 + j * 70
                out.append(f'<rect x="{x0+36}" y="{y}" width="{w}" height="26" rx="13" fill="#fff" fill-opacity="0.42"/>')
    # Fork (mast + carriage + tine) in copper, carrying a document.
    if small:
        mast = (262, 286, 72, 552)
        tine = (262, 776, 520, 62)
        doc = (366, 352, 312, 400)
    else:
        mast = (270, 300, 66, 532)
        tine = (270, 770, 480, 62)
        doc = (378, 420, 290, 340)
    mx, my, mw, mh = mast
    tx, ty, tw, th = tine
    out.append('<g filter="url(#lift)">')
    # Document (lifted payload) with a folded corner.
    dx, dy, dw, dh = doc
    f = 70 if not small else 90
    out.append(f'<path d="M{dx+18},{dy} H{dx+dw-f} L{dx+dw},{dy+f} V{dy+dh-18} Q{dx+dw},{dy+dh} {dx+dw-18},{dy+dh} '
               f'H{dx+18} Q{dx},{dy+dh} {dx},{dy+dh-18} V{dy+18} Q{dx},{dy} {dx+18},{dy} Z" fill="url(#paper)"/>')
    out.append(f'<path d="M{dx+dw-f},{dy} V{dy+f-14} Q{dx+dw-f},{dy+f} {dx+dw-f+14},{dy+f} H{dx+dw} Z" fill="#C9D8EE"/>')
    if not small:
        for j, w in enumerate((150, 196, 176, 120)):
            out.append(f'<rect x="{dx+38}" y="{dy+112+j*56}" width="{w}" height="22" rx="11" fill="#9DB7DD"/>')
        out.append(f'<rect x="{dx+38}" y="{dy+40}" width="96" height="40" rx="12" fill="#0A7CFF"/>')
    else:
        out.append(f'<rect x="{dx+46}" y="{dy+56}" width="130" height="58" rx="18" fill="#0A7CFF"/>')
    out.append('</g>')
    out.append('<g filter="url(#soft)">')
    out.append(f'<rect x="{mx}" y="{my}" width="{mw}" height="{mh}" rx="{mw/2}" fill="url(#copperV)"/>')
    # Tine: tapers to a point at the tip, like a real fork.
    out.append(f'<path d="M{tx+th/2},{ty} H{tx+tw-th*1.2} L{tx+tw},{ty+th*0.62} Q{tx+tw+4},{ty+th} {tx+tw-20},{ty+th} '
               f'H{tx+th/2} A{th/2},{th/2} 0 0 1 {tx+th/2},{ty} Z" fill="url(#copper)"/>')
    # Highlight on the mast.
    out.append(f'<rect x="{mx+mw*0.22}" y="{my+20}" width="{mw*0.18}" height="{mh-60}" rx="{mw*0.09}" fill="#fff" fill-opacity="0.35"/>')
    out.append('</g>')
    # Upward motion chevron above the document.
    cy = (dy - 96) if not small else (dy - 50)
    sw = 34 if not small else 44
    out.append(f'<path d="M{dx+dw/2-70},{cy+44} L{dx+dw/2},{cy-20} L{dx+dw/2+70},{cy+44}" fill="none" stroke="#fff" '
               f'stroke-width="{sw}" stroke-linecap="round" stroke-linejoin="round"/>' if not small else "")
    return svg(defs, "\n".join(out))


# ---------------------------------------------------------------- concept b: R


def concept_b(small):
    defs = blue_bg("bg", "#3D8BFF", "#1440B8") + """
  <linearGradient id="shaft" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#E9F1FC"/>
    <stop offset="1" stop-color="#D3E1F5"/>
  </linearGradient>"""
    out = [body("bg")]
    # Stem = a "lift shaft" pane with a copper up arrow; bowl = a file pane
    # stacked on its top right; leg = a white diagonal. Together: an "R".
    if small:
        sx0, sy0, sx1, sy1 = 214, 190, 470, 834
        bx0, by0, bx1, by1 = 496, 190, 812, 548
        leg = (590, 548, 812, 834, 128)
    else:
        sx0, sy0, sx1, sy1 = 252, 214, 452, 810
        bx0, by0, bx1, by1 = 474, 214, 778, 530
        leg = (586, 530, 780, 810, 108)
    r = (by1 - by0) / 2
    lx0, ly0, lx1, ly1, lw = leg
    out.append('<g filter="url(#soft)">')
    # Leg first, so the bowl overlaps its top: a slanted bar with round joins.
    out.append(f'<path d="M{lx0-lw/2},{ly0-80} H{lx0+lw/2} L{lx1},{ly1-14} H{lx1-lw*1.12} Z" fill="url(#paper)" '
               f'stroke="url(#paper)" stroke-width="28" stroke-linejoin="round"/>')
    out.append(f'<rect x="{sx0}" y="{sy0}" width="{sx1-sx0}" height="{sy1-sy0}" rx="40" fill="url(#paper)"/>')
    out.append(f'<path d="M{bx0+40},{by0} H{bx1-r} A{r},{r} 0 0 1 {bx1-r},{by1} H{bx0+40} '
               f'Q{bx0},{by1} {bx0},{by1-40} V{by0+40} Q{bx0},{by0} {bx0+40},{by0} Z" fill="url(#paper)"/>')
    out.append('</g>')
    # Inset shaft in the stem with the copper lift arrow.
    inset = 26 if not small else 34
    ix0, iy0, ix1, iy1 = sx0 + inset, sy0 + (78 if not small else inset), sx1 - inset, sy1 - inset
    out.append(f'<rect x="{ix0}" y="{iy0}" width="{ix1-ix0}" height="{iy1-iy0}" rx="24" fill="url(#shaft)"/>')
    cx = (ix0 + ix1) / 2
    hw = (ix1 - ix0) * (0.80 if not small else 0.86)
    sw = hw * 0.40
    top = iy0 + (34 if not small else 26)
    neck = top + hw * 0.72
    bot = iy1 - (40 if not small else 30)
    d = (f"M{cx},{top} L{cx+hw/2},{neck} H{cx+sw/2} V{bot} H{cx-sw/2} V{neck} H{cx-hw/2} Z")
    out.append(f'<g filter="url(#soft)"><path d="{d}" fill="url(#copperV)" stroke="url(#copperV)" stroke-width="14" stroke-linejoin="round"/></g>')
    out.append(f'<rect x="{cx-sw*0.28}" y="{neck+20}" width="{sw*0.16}" height="{bot-neck-40}" rx="{sw*0.08}" fill="#fff" fill-opacity="0.35"/>')
    if not small:
        for dx in (0, 26, 52):
            out.append(f'<circle cx="{sx0+34+dx}" cy="{sy0+40}" r="9" fill="#B9CBE6"/>')
            out.append(f'<circle cx="{bx0+34+dx}" cy="{by0+40}" r="9" fill="#B9CBE6"/>')
        for j, w in enumerate((176, 128, 152)):
            y = by0 + 104 + j * 62
            if j == 1:
                out.append(f'<rect x="{bx0+18}" y="{y-14}" width="{bx1-bx0-r*0.55}" height="52" rx="16" fill="#0A7CFF"/>')
                out.append(f'<rect x="{bx0+34}" y="{y}" width="{w}" height="24" rx="12" fill="#fff"/>')
            else:
                out.append(f'<rect x="{bx0+34}" y="{y}" width="{w}" height="24" rx="12" fill="#B9CBE6"/>')
    return svg(defs, "\n".join(out))


# ----------------------------------------------------------- concept c: Folder


def concept_c(small):
    defs = """
  <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#FBFCFE"/>
    <stop offset="1" stop-color="#D5DEEA"/>
  </linearGradient>
  <linearGradient id="back" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#3C97F5"/>
    <stop offset="1" stop-color="#1A6FD8"/>
  </linearGradient>
  <linearGradient id="front" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#7CC2FF"/>
    <stop offset="1" stop-color="#2F8CF2"/>
  </linearGradient>
  <linearGradient id="emboss" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#fff" stop-opacity="0.98"/>
    <stop offset="1" stop-color="#fff" stop-opacity="0.80"/>
  </linearGradient>
  <linearGradient id="cable" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#B4501F" stop-opacity="0"/>
    <stop offset="0.35" stop-color="#C8642E"/>
    <stop offset="1" stop-color="#E07B3F"/>
  </linearGradient>"""
    out = [body("bg")]
    # Lift cables from the top of the icon down to the platform.
    cx0, cx1 = (236, 788) if not small else (222, 802)
    cw = 14 if not small else 26
    if not small:
        for x in (cx0, cx1):
            out.append(f'<rect x="{x-cw/2}" y="100" width="{cw}" height="640" fill="url(#cable)" clip-path="url(#body)"/>')
    # Folder.
    fx0, fy0, fx1, fy1 = (232, 268, 792, 700) if not small else (176, 238, 848, 740)
    tab_w = 220 if not small else 270
    out.append('<g filter="url(#lift)">')
    out.append(f'<path d="M{fx0+30},{fy0} H{fx0+tab_w-30} Q{fx0+tab_w-8},{fy0} {fx0+tab_w+10},{fy0+22} L{fx0+tab_w+34},{fy0+50} '
               f'H{fx1-30} Q{fx1},{fy0+50} {fx1},{fy0+80} V{fy1-30} Q{fx1},{fy1} {fx1-30},{fy1} H{fx0+30} '
               f'Q{fx0},{fy1} {fx0},{fy1-30} V{fy0+30} Q{fx0},{fy0} {fx0+30},{fy0} Z" fill="url(#back)"/>')
    if not small:
        out.append(f'<rect x="{fx0+28}" y="{fy0+70}" width="{fx1-fx0-56}" height="80" rx="16" fill="#fff" fill-opacity="0.9"/>')
    ft = fy0 + (112 if not small else 120)
    out.append(f'<rect x="{fx0}" y="{ft}" width="{fx1-fx0}" height="{fy1-ft}" rx="30" fill="url(#front)"/>')
    out.append(f'<rect x="{fx0}" y="{ft}" width="{fx1-fx0}" height="4" fill="#fff" fill-opacity="0.6"/>')
    # Embossed up arrow on the folder front.
    acx, acy = (fx0 + fx1) / 2, (ft + fy1) / 2 + 4
    s = 1.0 if not small else 1.45
    arrow = [(0, -120), (110, -10), (44, -10), (44, 112), (-44, 112), (-44, -10), (-110, -10)]
    d = "M" + " L".join(f"{acx+x*s:.1f},{acy+y*s:.1f}" for x, y in arrow) + " Z"
    out.append(f'<path d="{d}" fill="#0B4FB0" fill-opacity="0.35" transform="translate(0 4)" stroke="#0B4FB0" '
               f'stroke-opacity="0.35" stroke-width="18" stroke-linejoin="round"/>')
    out.append(f'<path d="{d}" fill="url(#emboss)" stroke="#fff" stroke-width="18" stroke-linejoin="round" stroke-opacity="0.9"/>')
    out.append('</g>')
    # Copper lift platform under the folder.
    py = fy1 + (34 if not small else 26)
    ph = 40 if not small else 62
    out.append(f'<g filter="url(#soft)"><rect x="{fx0-30}" y="{py}" width="{fx1-fx0+60}" height="{ph}" rx="{ph/2}" fill="url(#copperV)"/>'
               f'<rect x="{fx0}" y="{py+6}" width="{fx1-fx0}" height="{ph*0.28}" rx="{ph*0.14}" fill="#fff" fill-opacity="0.35"/></g>')
    return svg(defs, "\n".join(out))


# ------------------------------------------------------------- DMG background


def dmg_background():
    """660x400 pt window background: a soft gradient, an arrow from the app to
    Applications and "Drag to Applications to install". Rendered at 1x and 2x."""
    return """<svg xmlns="http://www.w3.org/2000/svg" width="660" height="400" viewBox="0 0 660 400">
<defs>
  <linearGradient id="g" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#F7F9FC"/><stop offset="1" stop-color="#E3EAF3"/>
  </linearGradient>
  <linearGradient id="a" x1="268" y1="0" x2="398" y2="0" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#0A84FF" stop-opacity="0.35"/><stop offset="1" stop-color="#0A84FF"/>
  </linearGradient>
</defs>
<rect width="660" height="400" fill="url(#g)"/>
<path d="M268,190 H398" fill="none" stroke="url(#a)" stroke-width="10" stroke-linecap="round"/>
<path d="M378,170 L400,190 L378,210" fill="none" stroke="#0A84FF" stroke-width="10" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"""


# ---------------------------------------------------------------------- build


def renderer():
    exe = os.path.join(RENDER_DIR, "target", "release", "manta-icon-render")
    if not os.path.exists(exe):
        subprocess.run(["cargo", "build", "--release", "--quiet", "--manifest-path",
                        os.path.join(RENDER_DIR, "Cargo.toml")], check=True)
    return exe


def render(jobs):
    data = "\n".join(" ".join(map(str, j)) for j in jobs)
    subprocess.run([renderer(), "--batch"], input=data.encode(), check=True)


def font(size):
    for path in ("/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc"):
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            pass
    return ImageFont.load_default()


def contact_sheet(tmp):
    """All concepts at 1024 (shown at 256), 256, 64, 32 and 16 px, light and dark.
    Small sizes are shown 1:1 and also magnified 4x (nearest) to judge the pixels."""
    sizes = [256, 128, 64, 32, 16]
    pad, row_h = 40, 300
    width = pad + 260 + sum(s + 34 for s in sizes) + 2 * (64 + 34) + pad
    sheet = Image.new("RGB", (width, 80 + 2 * len(CONCEPTS) * row_h // 1 + pad), "#FFFFFF")
    d = ImageDraw.Draw(sheet)
    d.text((pad, 24), "Manta app icon concepts  (light and dark backgrounds; last two columns: 32 px x2 and 16 px x4, nearest-neighbour)",
           fill="#222", font=font(22))
    y = 80
    for bg_name, bg, fg in (("light", "#F2F2F7", "#222"), ("dark", "#1E1E1E", "#EEE")):
        for c, name in CONCEPTS.items():
            d.rectangle((0, y, width, y + row_h), fill=bg)
            d.text((pad, y + 16), f"{c})  {name}" + ("  (default)" if c == DEFAULT else ""), fill=fg, font=font(24))
            d.text((pad, y + 48), bg_name, fill="#888", font=font(16))
            x = pad + 260
            for s in sizes:
                im = Image.open(os.path.join(tmp, f"{c}_{s}.png")).convert("RGBA")
                sheet.paste(im, (x, y + (row_h - s) // 2), im)
                d.text((x, y + row_h - 26), f"{s}", fill="#888", font=font(14))
                x += s + 34
            for s in (32, 16):
                im = Image.open(os.path.join(tmp, f"{c}_{s}.png")).convert("RGBA").resize((64, 64), Image.NEAREST)
                sheet.paste(im, (x, y + (row_h - 64) // 2), im)
                d.text((x, y + row_h - 26), f"{s}px x{64 // s}", fill="#888", font=font(14))
                x += 64 + 34
            y += row_h
    return sheet


def main():
    # `--set <name>` delegates to packaging/icon/<name>/<name>_icons.py (iconkit pipeline),
    # e.g. `make_icons.py --set manta --icon a`. Without --set: the legacy first-generation concepts.
    if "--set" in sys.argv:
        i = sys.argv.index("--set")
        name = sys.argv[i + 1]
        rest = sys.argv[1:i] + sys.argv[i + 2:]
        sys.path.insert(0, os.path.join(HERE, name))
        mod = __import__(f"{name}_icons")
        from iconkit import main as kit_main
        kit_main(mod.SET, rest)
        return
    ap = argparse.ArgumentParser()
    ap.add_argument("--icon", choices=sorted(CONCEPTS), default=DEFAULT, help="concept to install as the app icon")
    args = ap.parse_args()

    os.makedirs(SRC, exist_ok=True)
    os.makedirs(CAND, exist_ok=True)
    makers = {"a": concept_a, "b": concept_b, "c": concept_c}
    for c, fn in makers.items():
        for small in (False, True):
            with open(os.path.join(SRC, f"{c}{'_small' if small else ''}.svg"), "w") as f:
                f.write(fn(small))
    with open(os.path.join(SRC, "dmg_background.svg"), "w") as f:
        f.write(dmg_background())

    with tempfile.TemporaryDirectory() as tmp:
        jobs = []
        for c in makers:
            big, small = os.path.join(SRC, f"{c}.svg"), os.path.join(SRC, f"{c}_small.svg")
            jobs.append((big, os.path.join(CAND, f"{c}_1024.png"), 1024))
            for s in (512, 256, 128, 64, 32, 16):
                # 16 and 32 px (and their @2x 32/64) use the simplified artwork.
                jobs.append((small if s <= 64 else big, os.path.join(tmp, f"{c}_{s}.png"), s))
            jobs.append((small, os.path.join(tmp, f"{c}_small_1024.png"), 1024))
        dmg_dir = os.path.join(ROOT, "packaging", "dmg")
        os.makedirs(dmg_dir, exist_ok=True)
        bg = os.path.join(SRC, "dmg_background.svg")
        jobs.append((bg, os.path.join(tmp, "bg1.png"), 660, 400))
        jobs.append((bg, os.path.join(tmp, "bg2.png"), 1320, 800))
        render(jobs)

        contact_sheet(tmp).save(os.path.join(CAND, "sheet.png"), optimize=True)

        # Caption (Pillow, since the resvg build has no text support).
        for name, k in (("bg1.png", 1), ("bg2.png", 2)):
            path = os.path.join(tmp, name)
            im = Image.open(path).convert("RGB")
            d = ImageDraw.Draw(im)
            text = "Drag Manta to Applications to install"
            f = font(15 * k)
            w = d.textlength(text, font=f)
            d.text(((660 * k - w) / 2, 312 * k), text, fill="#5A6472", font=f)
            im.save(path)
        # DMG background: one multi-resolution TIFF (1x + @2x) so Finder picks
        # the sharp one on Retina displays.
        tiff = os.path.join(dmg_dir, "background.tiff")
        if shutil.which("tiffutil"):
            subprocess.run(["tiffutil", "-cathidpicheck", os.path.join(tmp, "bg1.png"), os.path.join(tmp, "bg2.png"),
                            "-out", tiff], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        shutil.copy(os.path.join(tmp, "bg2.png"), os.path.join(dmg_dir, "background@2x.png"))
        shutil.copy(os.path.join(tmp, "bg1.png"), os.path.join(dmg_dir, "background.png"))

        c = args.icon
        shutil.copy(os.path.join(CAND, f"{c}_1024.png"), os.path.join(HERE, "AppIcon-1024.png"))
        shutil.copy(os.path.join(tmp, f"{c}_512.png"), os.path.join(ROOT, "crates", "app", "assets", "app-icon.png"))
        icns = os.path.join(HERE, "AppIcon.icns")
        if shutil.which("iconutil") is None:
            print("iconutil not found (macOS only): AppIcon.icns not updated", file=sys.stderr)
        else:
            iconset = os.path.join(tmp, "AppIcon.iconset")
            os.mkdir(iconset)
            for size in (16, 32, 128, 256, 512):
                for scale in (1, 2):
                    px = size * scale
                    src = os.path.join(tmp, f"{c}_{px}.png") if px != 1024 else os.path.join(CAND, f"{c}_1024.png")
                    name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
                    shutil.copy(src, os.path.join(iconset, name))
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", icns], check=True)
    for p in ("packaging/icon/candidates/sheet.png", "packaging/icon/AppIcon-1024.png",
              "packaging/icon/AppIcon.icns", "crates/app/assets/app-icon.png", "packaging/dmg/background.tiff"):
        print(p)


if __name__ == "__main__":
    main()
