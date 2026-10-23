"""iconkit: the shared, name-agnostic app-icon pipeline (SVG -> resvg -> sheet -> icns).

An icon *set* (packaging/icon/<set>/<set>_icons.py) supplies an IconSet: the app
name, its concepts (functions returning 1024 px SVG, with a simplified variant for
16/32 px), wordmark lettering and DMG colours. iconkit does the rest:

  <set>/src/<c>.svg, <c>_small.svg          SVG sources
  <set>/candidates/<c>_1024.png, sheet.png  candidates, contact sheet with mock Dock
  <set>/wordmark/<slug>-wordmark-<c>[-dark][@2x].png/.svg
  --install <c>: packaging/icon/AppIcon-1024.png, AppIcon.icns, VolumeIcon.icns,
                 crates/app/assets/app-icon.png, packaging/dmg/background*

The body is Apple's macOS grid: an 824 px continuous-corner squircle (corner radius
185.4) inside 1024 px with the standard soft drop shadow. Requires Pillow, cargo
(builds packaging/icon/render offline) and macOS iconutil/tiffutil.
"""
import argparse
import math
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from typing import Callable

from PIL import Image, ImageDraw, ImageFont

ICON_DIR = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(ICON_DIR, "..", ".."))
RENDER_DIR = os.path.join(ICON_DIR, "render")
SS = 4  # supersampling factor for every raster

# ------------------------------------------------------------------ geometry


def squircle(x0=100.0, y0=100.0, x1=924.0, y1=924.0, r=185.4):
    """Apple's continuous-corner rounded rect (the iOS/macOS icon shape)."""
    k = [((1.08849323, 0), (0.86840689, 0.02229018), (0.66993427, 0.06549600)),
         ((0.37282392, 0.16232684), (0.16232684, 0.37282392), (0.06549600, 0.66993427)),
         ((0.02229018, 0.86840689), (0, 1.08849323), (0, 1.52866471))]
    corners = [lambda a, b: (x1 - a * r, y0 + b * r), lambda a, b: (x1 - b * r, y1 - a * r),
               lambda a, b: (x0 + a * r, y1 - b * r), lambda a, b: (x0 + b * r, y0 + a * r)]
    s = 1.52866471
    d = [f"M{x0 + s * r:.2f},{y0:.2f}"]
    for P in corners:
        d.append("L%.2f,%.2f" % P(s, 0))
        for seg in k:
            d.append("C" + " ".join("%.2f,%.2f" % P(a, b) for a, b in seg))
    return " ".join(d) + " Z"


SQ = squircle()


def rpoly(pts, r):
    """Closed polygon with every corner rounded by radius r (quadratic joins)."""
    if isinstance(r, (int, float)):
        r = [r] * len(pts)
    n = len(pts)
    d = []
    for i in range(n):
        p0, p1, p2 = pts[i - 1], pts[i], pts[(i + 1) % n]
        v0 = (p0[0] - p1[0], p0[1] - p1[1])
        v2 = (p2[0] - p1[0], p2[1] - p1[1])
        l0, l2 = math.hypot(*v0), math.hypot(*v2)
        rr = min(r[i], l0 / 2, l2 / 2)
        a = (p1[0] + v0[0] / l0 * rr, p1[1] + v0[1] / l0 * rr)
        b = (p1[0] + v2[0] / l2 * rr, p1[1] + v2[1] / l2 * rr)
        d.append(("M" if i == 0 else "L") + f"{a[0]:.2f},{a[1]:.2f} Q{p1[0]:.2f},{p1[1]:.2f} {b[0]:.2f},{b[1]:.2f}")
    return " ".join(d) + " Z"


def quad_pt(q, u, v):
    """Bilinear point in quad q = (tl, tr, br, bl) at (u, v) in [0,1]^2."""
    tl, tr, br, bl = q
    top = (tl[0] + (tr[0] - tl[0]) * u, tl[1] + (tr[1] - tl[1]) * u)
    bot = (bl[0] + (br[0] - bl[0]) * u, bl[1] + (br[1] - bl[1]) * u)
    return (top[0] + (bot[0] - top[0]) * v, top[1] + (bot[1] - top[1]) * v)


def quad_sub(q, u0, v0, u1, v1):
    return [quad_pt(q, u0, v0), quad_pt(q, u1, v0), quad_pt(q, u1, v1), quad_pt(q, u0, v1)]


def rrect(x0, y0, x1, y1, rl, rr=None):
    """Rounded rect with separate left/right corner radii."""
    rr = rl if rr is None else rr
    return (f"M{x0 + rl},{y0} H{x1 - rr} Q{x1},{y0} {x1},{y0 + rr} V{y1 - rr} Q{x1},{y1} {x1 - rr},{y1} "
            f"H{x0 + rl} Q{x0},{y1} {x0},{y1 - rl} V{y0 + rl} Q{x0},{y0} {x0 + rl},{y0} Z")


def rrect_c(x0, y0, x1, y1, rl, rr=None):
    """Like rrect but with circular arcs (crisper large radii)."""
    rr = rl if rr is None else rr
    return (f"M{x0 + rl},{y0} H{x1 - rr} A{rr},{rr} 0 0 1 {x1},{y0 + rr} V{y1 - rr} A{rr},{rr} 0 0 1 {x1 - rr},{y1} "
            f"H{x0 + rl} A{rl},{rl} 0 0 1 {x0},{y1 - rl} V{y0 + rl} A{rl},{rl} 0 0 1 {x0 + rl},{y0} Z")


# ------------------------------------------------------------- shared parts

COMMON_DEFS = f"""
  <filter id="dropshadow" x="-20%" y="-20%" width="140%" height="140%">
    <feGaussianBlur stdDeviation="12"/>
  </filter>
  <filter id="ambient" x="-20%" y="-20%" width="140%" height="140%">
    <feGaussianBlur stdDeviation="3"/>
  </filter>
  <filter id="soft" x="-30%" y="-30%" width="160%" height="160%">
    <feGaussianBlur in="SourceAlpha" stdDeviation="10"/>
    <feOffset dy="10"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.30"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="lift" x="-30%" y="-30%" width="160%" height="160%">
    <feGaussianBlur in="SourceAlpha" stdDeviation="16"/>
    <feOffset dy="18"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.34"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="glow" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="18"/></filter>
  <filter id="glow8" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="8"/></filter>
  <filter id="frost" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="30"/></filter>
  <clipPath id="body"><path d="{SQ}"/></clipPath>
  <linearGradient id="rim" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#fff" stop-opacity="0.60"/>
    <stop offset="0.22" stop-color="#fff" stop-opacity="0.10"/>
    <stop offset="0.75" stop-color="#000" stop-opacity="0.0"/>
    <stop offset="1" stop-color="#000" stop-opacity="0.22"/>
  </linearGradient>
  <radialGradient id="sheen" cx="512" cy="60" r="700" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#fff" stop-opacity="0.26"/>
    <stop offset="1" stop-color="#fff" stop-opacity="0"/>
  </radialGradient>
"""


def body(fill, sheen=True):
    """Apple-grid body: ambient + drop shadow, filled squircle, sheen, rim light."""
    return f"""
  <path d="{SQ}" fill="#000" fill-opacity="0.20" filter="url(#ambient)" transform="translate(0 2)"/>
  <path d="{SQ}" fill="#000" fill-opacity="0.30" filter="url(#dropshadow)" transform="translate(0 10)"/>
  <path d="{SQ}" fill="{fill}"/>
  {f'<path d="{SQ}" fill="url(#sheen)"/>' if sheen else ''}
  <path d="{SQ}" fill="none" stroke="url(#rim)" stroke-width="5" clip-path="url(#body)"/>"""


def svg(defs, content, w=1024, h=1024):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<defs>{COMMON_DEFS}{defs}
</defs>{content}
</svg>
"""


def lin(id_, stops, x1=0, y1=0, x2=0, y2=1, user=False):
    units = ' gradientUnits="userSpaceOnUse"' if user else ""
    s = "".join(f'<stop offset="{o}" stop-color="{c}"' + (f' stop-opacity="{a}"' if a is not None else "") + "/>"
                for o, c, a in [(t + (None,))[:3] for t in stops])
    return f'\n  <linearGradient id="{id_}" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"{units}>{s}</linearGradient>'


def P(pts):
    return "M" + " L".join(f"{x:.2f},{y:.2f}" for x, y in pts) + " Z"



# ------------------------------------------------------------- neighbours

NEIGHBOURS = [  # generic placeholders: (top, bottom, glyph colour, glyph)
    ("#5AB8FF", "#1673E6", "#fff", "circle"),
    ("#F7F7F9", "#D9DADF", "#8E8E93", "bars"),
    ("#9EA3AD", "#5D626C", "#fff", "ring"),
    ("#6EE38A", "#21A846", "#fff", "rect"),
    ("#3A3A3E", "#121214", "#30D158", "bars"),
    ("#FF7A6E", "#E5332B", "#fff", "circle"),
    ("#C58BFF", "#8C3FE0", "#fff", "ring"),
]


def neighbour_svg(top, bot, ink, glyph):
    g = {"circle": f'<circle cx="512" cy="512" r="190" fill="{ink}"/>',
         "ring": f'<circle cx="512" cy="512" r="200" fill="none" stroke="{ink}" stroke-width="70"/>',
         "rect": f'<rect x="300" y="330" width="424" height="364" rx="60" fill="{ink}"/>',
         "bars": "".join(f'<rect x="300" y="{330 + i * 120}" width="{424 - i * 90}" height="64" rx="32" fill="{ink}"/>'
                         for i in range(3))}[glyph]
    return svg(lin("bg", [(0, top), (1, bot)]), body("url(#bg)") + g)




# ------------------------------------------------------------------ settings


@dataclass
class Lettering:
    """Stroked custom lettering: fn() -> (parts, advance) on a baseline at y=0.
    parts are SVG path strings (stroked) or ("dot", x, y) tuples (filled circles)."""
    fn: Callable
    cap: float = 220          # cap height, for vertical centring
    stroke: float = 38        # stroke width
    scale: float = 0.86       # lettering scale inside the 360 px tall wordmark


@dataclass
class IconSet:
    name: str                         # "Manta"
    here: str                         # directory of the set (packaging/icon/<set>)
    concepts: dict                    # key -> (title, maker(small: bool) -> svg str)
    lettering: Lettering
    recommended: str = ""
    small_max: int = 32               # sizes <= this use the simplified artwork
    ink_light: str = "#1C1A2E"        # wordmark text on light backgrounds
    ink_dark: str = "#FFFFFF"         # ... on dark backgrounds
    dmg_bg: tuple = ("#FBFAFE", "#ECE9F5")
    dmg_accent: str = "#6B5CF0"
    dmg_caption: str = ""             # empty: none
    dmg_caption_ink: str = "#5E5A72"
    icon_pad: dict = field(default_factory=dict)

    @property
    def slug(self):
        return self.name.lower()

    @property
    def src(self):
        return os.path.join(self.here, "src")

    @property
    def cand(self):
        return os.path.join(self.here, "candidates")

    @property
    def word(self):
        return os.path.join(self.here, "wordmark")

    def maker(self, c):
        return self.concepts[c][1]


# ------------------------------------------------------------------ build


def renderer():
    exe = os.path.join(RENDER_DIR, "target", "release", "manta-icon-render")
    if not os.path.exists(exe):
        subprocess.run(["cargo", "build", "--release", "--quiet", "--offline", "--manifest-path",
                        os.path.join(RENDER_DIR, "Cargo.toml")], check=True)
    return exe


def render(jobs):
    data = "\n".join(" ".join(map(str, j)) for j in jobs)
    subprocess.run([renderer(), "--batch"], input=data.encode(), check=True)


def downsample(path, w, h=None):
    """Filter a supersampled raster (premultiplied alpha) down to its final size, in place."""
    h = h or w
    im = Image.open(path).convert("RGBa").resize((w, h), Image.LANCZOS if w >= 128 else Image.BOX)
    im.convert("RGBA").save(path)


def font(size):
    for path in ("/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc"):
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            pass
    return ImageFont.load_default()


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


def build_sources(S):
    os.makedirs(S.src, exist_ok=True)
    for c in S.concepts:
        write(os.path.join(S.src, f"{c}.svg"), S.maker(c)(False))
        write(os.path.join(S.src, f"{c}_small.svg"), S.maker(c)(True))
    for i, n in enumerate(NEIGHBOURS):
        write(os.path.join(S.src, f"_neighbour{i}.svg"), neighbour_svg(*n))


def render_sizes(S, tmp, concepts, sizes=(1024, 512, 256, 128, 96, 64, 32, 16)):
    """<tmp>/<c>_<s>.png for each concept and size, supersampled."""
    jobs, outs = [], []
    for c in concepts:
        for s in sizes:
            src = os.path.join(S.src, f"{c}_small.svg" if s <= S.small_max else f"{c}.svg")
            out = os.path.join(tmp, f"{c}_{s}.png")
            jobs.append((src, out, s * SS if s <= 512 else s * 2))
            outs.append((out, s))
    render(jobs)
    for out, s in outs:
        downsample(out, s)


def _vgrad(w, h, top, bot):
    im = Image.new("RGBA", (w, h))
    d = ImageDraw.Draw(im)
    t, b = [tuple(int(x[i:i + 2], 16) for i in (1, 3, 5)) for x in (top, bot)]
    for y in range(h):
        k = y / max(1, h - 1)
        d.line([(0, y), (w, y)], fill=tuple(int(t[j] + (b[j] - t[j]) * k) for j in range(3)) + (255,))
    return im


def dock_strip(tmp, c, dark, n=96):
    """A mock Dock: the concept between generic placeholder icons, on a wallpaper."""
    icons = [os.path.join(tmp, f"_n{i}_{n}.png") for i in range(len(NEIGHBOURS))]
    order = icons[:3] + [os.path.join(tmp, f"{c}_{n}.png")] + icons[3:]
    gap = 14
    w, h = len(order) * (n + gap) + gap + 40, n + 60
    im = _vgrad(w, h, *(("#F4E9F2", "#C9D9F2") if not dark else ("#1B2236", "#090B12")))
    shelf = Image.new("RGBA", (w, h))
    ImageDraw.Draw(shelf).rounded_rectangle(
        (20, 14, w - 20, h - 10), 28,
        fill=(255, 255, 255, 110) if not dark else (60, 60, 70, 150),
        outline=(255, 255, 255, 140) if not dark else (255, 255, 255, 40))
    im = Image.alpha_composite(im, shelf)
    x = 20 + gap
    for p in order:
        im.alpha_composite(Image.open(p).convert("RGBA"), (x, 22))
        x += n + gap
    cx = 20 + gap + 3 * (n + gap) + n // 2
    ImageDraw.Draw(im).ellipse((cx - 3, h - 18, cx + 3, h - 12),
                               fill=(40, 40, 40, 200) if not dark else (230, 230, 230, 220))
    return im


def contact_sheet(S, tmp):
    sizes = [256, 128, 64, 32, 16]
    pad, row_h, label_w = 40, 300, 230
    block_w = label_w + sum(s + 30 for s in sizes) + 2 * (64 + 30)
    width = pad + 2 * block_w + pad
    dock_h = 186
    n = len(S.concepts)
    sheet = Image.new("RGB", (width, 90 + n * row_h + 60 + n * dock_h + pad), "#FFFFFF")
    d = ImageDraw.Draw(sheet)
    d.text((pad, 22), f"{S.name} app icon concepts", fill="#111", font=font(30))
    d.text((pad, 60), "Left: light background, right: dark. Columns: 256, 128, 64, 32, 16 px (1:1), then 32 px x2 "
                      f"and 16 px x4 (nearest). Sizes <= {S.small_max} px use the simplified artwork.",
           fill="#666", font=font(16))
    y = 90
    for c, (title, _) in S.concepts.items():
        for side, (bg, fg) in enumerate((("#F2F2F7", "#222"), ("#1E1E1E", "#EEE"))):
            x0 = pad + side * block_w
            d.rectangle((x0, y, x0 + block_w - 10, y + row_h - 8), fill=bg)
            d.text((x0 + 18, y + 18), f"{c})  {title}" + ("   *" if c == S.recommended else ""), fill=fg, font=font(26))
            x = x0 + label_w
            for s in sizes:
                im = Image.open(os.path.join(tmp, f"{c}_{s}.png")).convert("RGBA")
                sheet.paste(im, (x, y + (row_h - 8 - s) // 2), im)
                d.text((x, y + row_h - 34), f"{s}", fill="#888", font=font(14))
                x += s + 30
            for s in (32, 16):
                im = Image.open(os.path.join(tmp, f"{c}_{s}.png")).convert("RGBA").resize((64, 64), Image.NEAREST)
                sheet.paste(im, (x, y + (row_h - 8 - 64) // 2), im)
                d.text((x, y + row_h - 34), f"{s}x{64 // s}", fill="#888", font=font(14))
                x += 64 + 30
        y += row_h
    d.text((pad, y + 16), "Mock Dock (96 px, generic placeholder neighbours; the dot marks the concept)",
           fill="#111", font=font(22))
    y += 60
    for c in S.concepts:
        for side, dark in enumerate((False, True)):
            strip = dock_strip(tmp, c, dark)
            sheet.paste(strip, (pad + side * block_w, y), strip)
        d.text((pad + block_w - 60, y + 60), c, fill="#222", font=font(26))
        y += dock_h
    return sheet


def wordmark_svg(S, concept, ink):
    """Icon + lettering on a transparent canvas 360 px tall (1x = 180 px)."""
    icon = S.maker(concept)(False).replace(
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024"',
        '<svg x="0" y="0" width="360" height="360"', 1)
    L = S.lettering
    parts, adv = L.fn()
    s = L.scale
    tx, ty = 372, 180 + L.cap * s / 2
    g = [f'<g transform="translate({tx} {ty:.1f}) scale({s})" fill="none" stroke="{ink}" stroke-width="{L.stroke}" '
         f'stroke-linecap="round" stroke-linejoin="round">']
    for p in parts:
        if isinstance(p, tuple):
            g.append(f'<circle cx="{p[1]}" cy="{p[2]}" r="{L.stroke / 2 + 3}" fill="{ink}" stroke="none"/>')
        else:
            g.append(f'<path d="{p}"/>')
    g.append("</g>")
    w = int(math.ceil(tx + adv * s + 24))
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="360" viewBox="0 0 {w} 360">\n'
            f'{icon}\n{"".join(g)}\n</svg>\n'), w


def build_wordmarks(S, concepts):
    os.makedirs(S.word, exist_ok=True)
    jobs, post = [], []
    for c in concepts:
        for suffix, ink in (("", S.ink_light), ("-dark", S.ink_dark)):
            text, w = wordmark_svg(S, c, ink)
            path = os.path.join(S.word, f"{S.slug}-wordmark-{c}{suffix}.svg")
            write(path, text)
            for scale, tag in ((0.5, ""), (1, "@2x")):
                out = os.path.join(S.word, f"{S.slug}-wordmark-{c}{suffix}{tag}.png")
                ow, oh = int(round(w * scale)), int(round(360 * scale))
                jobs.append((path, out, ow * SS, oh * SS))
                post.append((out, ow, oh))
    render(jobs)
    for out, ow, oh in post:
        downsample(out, ow, oh)


def dmg_background_svg(S, c):
    """660x400 installer window: wordmark on top, a drag arrow between app and Applications."""
    text, w = wordmark_svg(S, c, S.ink_light)
    inner = text.split("\n", 1)[1].rsplit("</svg>", 1)[0]
    s = 60 / 360
    top, bot = S.dmg_bg
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="660" height="400" viewBox="0 0 660 400">
<defs>
  <linearGradient id="dg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bot}"/></linearGradient>
  <linearGradient id="da" x1="268" y1="0" x2="398" y2="0" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="{S.dmg_accent}" stop-opacity="0.25"/><stop offset="1" stop-color="{S.dmg_accent}"/></linearGradient>
</defs>
<rect width="660" height="400" fill="url(#dg)"/>
<svg x="{(660 - w * s) / 2:.1f}" y="28" width="{w * s:.1f}" height="60" viewBox="0 0 {w} 360">{inner}</svg>
<path d="M268,190 H398" fill="none" stroke="url(#da)" stroke-width="10" stroke-linecap="round"/>
<path d="M378,170 L400,190 L378,210" fill="none" stroke="{S.dmg_accent}" stroke-width="10" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"""


def render_dmg_background(S, c, tmp):
    """Returns [(1x png), (2x png)] in tmp."""
    bg = os.path.join(tmp, "dmg_background.svg")
    write(bg, dmg_background_svg(S, c))
    pngs = []
    for k in (1, 2):
        p = os.path.join(tmp, f"dmg_bg{k}.png")
        render([(bg, p, 660 * k * SS, 400 * k * SS)])
        downsample(p, 660 * k, 400 * k)
        im = Image.open(p).convert("RGB")
        if S.dmg_caption:  # caption via Pillow: the resvg build has no text support
            dr = ImageDraw.Draw(im)
            f = font(14 * k)
            dr.text(((660 * k - dr.textlength(S.dmg_caption, font=f)) / 2, 316 * k), S.dmg_caption,
                    fill=S.dmg_caption_ink, font=f)
        im.save(p)
        pngs.append(p)
    return pngs


def install(S, c, tmp):
    """Make concept c the app icon, window icon, DMG volume icon and DMG background."""
    render_sizes(S, tmp, [c], sizes=(1024, 512, 256, 128, 64, 32, 16))
    shutil.copy(os.path.join(tmp, f"{c}_1024.png"), os.path.join(ICON_DIR, "AppIcon-1024.png"))
    assets = os.path.join(ROOT, "crates", "app", "assets")
    os.makedirs(assets, exist_ok=True)
    shutil.copy(os.path.join(tmp, f"{c}_512.png"), os.path.join(assets, "app-icon.png"))
    written = ["packaging/icon/AppIcon-1024.png", "crates/app/assets/app-icon.png"]
    if shutil.which("iconutil") is None:
        print("iconutil not found (macOS only): .icns files not updated", file=sys.stderr)
    else:
        iconset = os.path.join(tmp, "AppIcon.iconset")
        os.mkdir(iconset)
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
                shutil.copy(os.path.join(tmp, f"{c}_{size * scale}.png"), os.path.join(iconset, name))
        for name in ("AppIcon.icns", "VolumeIcon.icns"):
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(ICON_DIR, name)], check=True)
            written.append(f"packaging/icon/{name}")
    dmg_dir = os.path.join(ROOT, "packaging", "dmg")
    os.makedirs(dmg_dir, exist_ok=True)
    p1, p2 = render_dmg_background(S, c, tmp)
    shutil.copy(p1, os.path.join(dmg_dir, "background.png"))
    shutil.copy(p2, os.path.join(dmg_dir, "background@2x.png"))
    if shutil.which("tiffutil"):
        subprocess.run(["tiffutil", "-cathidpicheck", p1, p2, "-out", os.path.join(dmg_dir, "background.tiff")],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return written + ["packaging/dmg/background.png", "packaging/dmg/background@2x.png", "packaging/dmg/background.tiff"]


def main(S, argv=None):
    keys = sorted(S.concepts)
    ap = argparse.ArgumentParser(description=f"{S.name} app icon concepts")
    ap.add_argument("--install", "--icon", dest="install", choices=keys,
                    help="make this concept the app icon (AppIcon.icns, window icon, DMG volume icon and background)")
    ap.add_argument("--only", choices=keys, help="render only this concept (quick iteration, into a preview dir)")
    ap.add_argument("--preview", help="with --only: also write 1024/128/32/16 px and Dock strips into this directory")
    args = ap.parse_args(argv)

    build_sources(S)
    os.makedirs(S.cand, exist_ok=True)
    concepts = [args.only] if args.only else list(S.concepts)
    with tempfile.TemporaryDirectory() as tmp:
        render_sizes(S, tmp, concepts)
        for c in concepts:
            shutil.copy(os.path.join(tmp, f"{c}_1024.png"), os.path.join(S.cand, f"{c}_1024.png"))
        jobs = [(os.path.join(S.src, f"_neighbour{i}.svg"), os.path.join(tmp, f"_n{i}_96.png"), 96 * SS)
                for i in range(len(NEIGHBOURS))]
        render(jobs)
        for i in range(len(NEIGHBOURS)):
            downsample(os.path.join(tmp, f"_n{i}_96.png"), 96)
        if args.only and args.preview:
            os.makedirs(args.preview, exist_ok=True)
            c = args.only
            prev = _vgrad(1100, 700, "#F2F2F7", "#F2F2F7")
            dark = _vgrad(1100, 700, "#1E1E1E", "#1E1E1E")
            canvas = Image.new("RGBA", (1100, 1400))
            canvas.paste(prev, (0, 0)); canvas.paste(dark, (0, 700))
            for row in (0, 1):
                x = 20
                for s in (512, 128, 64, 32, 16):
                    im = Image.open(os.path.join(tmp, f"{c}_{s}.png")).convert("RGBA")
                    canvas.alpha_composite(im, (x, row * 700 + 20))
                    x += s + 20
                x = 20 + 512 + 20
                for s in (32, 16):
                    im = Image.open(os.path.join(tmp, f"{c}_{s}.png")).convert("RGBA").resize((128, 128), Image.NEAREST)
                    canvas.alpha_composite(im, (x, row * 700 + 200))
                    x += 148
                strip = dock_strip(tmp, c, bool(row))
                canvas.alpha_composite(strip, (20, row * 700 + 540))
            canvas.save(os.path.join(args.preview, f"{c}_preview.png"))
        if not args.only:
            contact_sheet(S, tmp).save(os.path.join(S.cand, "sheet.png"), optimize=True)
            build_wordmarks(S, concepts)
            c = S.recommended or concepts[0]
            p1, p2 = render_dmg_background(S, c, tmp)
            shutil.copy(p1, os.path.join(S.word, "dmg-background-preview.png"))
            shutil.copy(p2, os.path.join(S.word, "dmg-background-preview@2x.png"))
            print(os.path.relpath(os.path.join(S.cand, "sheet.png"), ROOT))
        if args.install:
            for p in install(S, args.install, tmp):
                print(p)
