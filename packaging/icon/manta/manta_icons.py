#!/usr/bin/env python3
"""Manta app icon: four concepts authored as SVG, rasterised with resvg.

    python3 packaging/icon/manta/manta_icons.py              # sources, candidates, sheet, wordmarks, DMG preview
    python3 packaging/icon/manta/manta_icons.py --install a  # ...and make concept a the app icon
    python3 packaging/icon/make_icons.py --set manta --icon a  # same as the line above
    python3 packaging/icon/manta/manta_icons.py --only b --preview /tmp/p   # quick look at one concept

A manta ray glides: calm, effortless and wide. Its two broad wings are the two panes
of the file manager. The shared pipeline (squircle grid, rendering, contact sheet,
Dock strip, .icns, DMG background) is packaging/icon/iconkit.py; this file holds the
concepts, the lettering and the settings block (SET).

  a  "Glide"     a pale manta gliding diagonally through a deep ocean (midnight -> teal),
                 light rays from above; each wing carries a faint pane of file rows
  b  "Panes"     a symmetrical, geometric manta whose wings are two glass panes meeting
                 at the body; the cephalic fins form a soft "M"; bioluminescent aqua on ink
  c  "Monogram"  a bold "M" that is a manta seen from above: the cephalic fins are the
                 two peaks, the swept wings the outer legs, the tail a flowing accent;
                 sunset coral -> magenta
  d  "Surface"   a manta just under a sunlit water surface with caustic light; teal/mint
"""
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from iconkit import IconSet, Lettering, body, lin, main, rpoly, svg  # noqa: E402

# ------------------------------------------------------------ manta outline


def _fmt(p):
    return f"{p[0]:.2f},{p[1]:.2f}"


def manta_half(lobe=0.45, tail=420, tip=(372, 64), small=False):
    """Right half of a manta seen from above, facing up (-y), body centred on x=0.
    Returns (start, [(c1, c2, end), ...]); the last point is the tail tip.
    The cephalic fins are short, rounded paddles that curl slightly inwards, so the
    animal reads as a friendly manta (not horns, not a bat)."""
    L = lobe
    tx, ty = tip
    segs = [
        ((20, -148), (38, -150), (50, -154)),                                 # broad mouth edge
        ((48, -170 - 20 * L), (50, -186 - 34 * L), (64, -190 - 36 * L)),     # cephalic fin, inner side
        ((82, -194 - 36 * L), (100, -178 - 26 * L), (96, -158)),             # rounded paddle tip
        ((93, -146), (104, -136), (128, -132)),                               # head to wing root
        ((222, -122), (tx - 70, -56), (tx - 12, ty - 46)),                    # leading edge
        ((tx + 6, ty - 22), (tx + 8, ty - 4), (tx, ty)),                      # falcate wing tip
        ((tx - 70, ty - 8), (200, 62), (128, 112)),                           # trailing edge
        ((100, 132), (66, 150), (34, 168)),                                   # rear body
        ((22, 176), (14, 184), (10, 192)),                                    # pelvic fin
    ]
    if tail:
        w = 7 if not small else 14
        segs.append(((w + 1, 192 + tail * 0.3), (w * 0.5, 192 + tail * 0.7), (w * 0.25, 192 + tail)))
    return (0, -144), segs


def manta_path(**kw):
    start, segs = manta_half(**kw)
    d = [f"M{_fmt(start)}"]
    for c1, c2, e in segs:
        d.append(f"C{_fmt(c1)} {_fmt(c2)} {_fmt(e)}")
    # Mirror: walk the right half backwards with x negated.
    pts = [start] + [e for _, _, e in segs]
    rev = []
    for i in range(len(segs) - 1, -1, -1):
        c1, c2, e = segs[i]
        p0 = pts[i]
        rev.append(((-c2[0], c2[1]), (-c1[0], c1[1]), (-p0[0], p0[1])))
    last = segs[-1][2]
    d.append(f"L{_fmt((-last[0], last[1]))}")
    for c1, c2, e in rev:
        d.append(f"C{_fmt(c1)} {_fmt(c2)} {_fmt(e)}")
    return " ".join(d) + " Z"


# ------------------------------------------------------------ concept a: Glide


def wing_rows(ink, op, sgn, rows=((150, 130), (152, 104), (156, 76)), y0=-92, dy=34, dots=True):
    """A list pane on one wing, in manta space: bullet + bar rows running out along the span."""
    g = []
    for j, (x, w) in enumerate(rows):
        y = y0 + j * dy
        g.append(f'<circle cx="{sgn * (x + 6)}" cy="{y + 7}" r="7" fill="{ink}" fill-opacity="{op * 1.3}"/>')
        x1 = x + 24
        g.append(f'<rect x="{x1 if sgn > 0 else -(x1 + w)}" y="{y}" width="{w}" height="14" rx="7" '
                 f'fill="{ink}" fill-opacity="{op}"/>')
    return "".join(g)


def concept_a(small):
    """A pale manta gliding diagonally through a deep ocean, light rays from above."""
    defs = (lin("bg", [(0, "#13809A"), (0.45, "#0B4572"), (1, "#081A3C")], 0.3, 0, 0.6, 1)
            + lin("ray", [(0, "#BDF4FF", 0.55), (1, "#BDF4FF", 0)], 0, 0, 0, 1)
            + lin("skin", [(0, "#FFFFFF"), (0.55, "#E6FBFF"), (1, "#A6E6F4")], 0, 0, 0, 1)
            + lin("wingl", [(0, "#0B3F6E", 0.0), (1, "#0B3F6E", 0.10)], 1, 0, 0, 0)
            + lin("wingr", [(0, "#0B3F6E", 0.04), (1, "#0B3F6E", 0.24)], 0, 0, 1, 0))
    out = [body("url(#bg)")]
    if not small:
        rays = []
        for x, w, op in ((290, 90, 0.35), (460, 60, 0.25), (600, 120, 0.30), (770, 70, 0.2)):
            rays.append(f'<path d="M{x - w / 2},90 L{x + w / 2},90 L{x + w * 1.6 - 120},760 L{x - w * 0.6 - 160},760 Z" '
                        f'fill="url(#ray)" opacity="{op}"/>')
        out.append(f'<g clip-path="url(#body)" filter="url(#glow8)">{"".join(rays)}</g>')
    tr = "translate(516 528) rotate(-24) scale(0.86)" if not small else "translate(512 512) rotate(-24) scale(1.06)"
    mp = manta_path(tail=250 if not small else 70, small=small)
    defs += f'\n  <clipPath id="mclip"><path d="{mp}"/></clipPath>'
    out.append(f'<g clip-path="url(#body)"><g transform="{tr}">')
    out.append(f'<path d="{mp}" fill="#02101F" fill-opacity="0.45" filter="url(#glow)" transform="translate(-30 50)"/>')
    out.append(f'<path d="{mp}" fill="url(#skin)"/>')
    out.append('<g clip-path="url(#mclip)">')
    out.append('<rect x="-400" y="-300" width="400" height="800" fill="url(#wingl)"/>')
    out.append('<rect x="0" y="-300" width="400" height="800" fill="url(#wingr)"/>')
    if not small:
        out.append(wing_rows("#0B4572", 0.13, -1) + wing_rows("#0B4572", 0.13, 1))
        out.append('<path d="M0,-130 C8,-60 8,60 0,160" stroke="#fff" stroke-opacity="0.85" stroke-width="14" '
                   'stroke-linecap="round" fill="none" filter="url(#glow8)"/>')
    out.append('</g>')
    out.append(f'<path d="{mp}" fill="none" stroke="#fff" stroke-opacity="0.9" stroke-width="{3 if not small else 6}"/>')
    out.append('</g></g>')
    return svg(defs, "\n".join(out))


# ------------------------------------------------------------ concept b: Panes


def concept_b(small):
    """Symmetrical manta whose wings are two glass panes meeting at a glowing spine."""
    defs = (lin("bg", [(0, "#16264E"), (1, "#060A1A")], 0.3, 0, 0.7, 1)
            + lin("paneL", [(0, "#7DFFE2", 0.62), (1, "#17A6C4", 0.28)], 1, 0, 0, 1)
            + lin("paneR", [(0, "#5FE6FF", 0.62), (1, "#1B6FD0", 0.28)], 0, 0, 1, 1)
            + lin("spine", [(0, "#F2FFFE"), (1, "#8CEFF8")], 0, 0, 0, 1))
    gap = 22 if not small else 34
    tr = "translate(512 494) scale(0.9)" if not small else "translate(512 480) scale(1.02)"
    mp = manta_path(tail=0, small=small)
    defs += (f'\n  <clipPath id="mc"><path d="{mp}"/></clipPath>'
             f'\n  <clipPath id="hl"><rect x="-500" y="-400" width="{500 - gap}" height="900"/></clipPath>'
             f'\n  <clipPath id="hr"><rect x="{gap}" y="-400" width="{500 - gap}" height="900"/></clipPath>')
    out = [body("url(#bg)")]
    out.append(f'<g clip-path="url(#body)"><g transform="{tr}">')
    # Bioluminescent halo.
    out.append(f'<path d="{mp}" fill="#22D8F0" fill-opacity="0.45" filter="url(#glow)"/>')
    sw = 7 if not small else 14
    for side, fill in (("hl", "paneL"), ("hr", "paneR")):
        x = -gap if side == "hl" else gap
        out.append(f'<g clip-path="url(#{side})"><g clip-path="url(#mc)">'
                   f'<rect x="-500" y="-400" width="1000" height="900" fill="url(#{fill})"/>'
                   + ('' if small else
                      f'<g transform="translate(0 6)">{wing_rows("#E9FFFF", 0.55, 1 if side == "hr" else -1, y0=-70, dy=36)}</g>'
                      + "".join(f'<circle cx="{(1 if side == "hr" else -1) * (150 + k * 24)}" cy="-104" r="7" fill="#E9FFFF" fill-opacity="0.85"/>'
                                for k in range(3)))
                   + f'<path d="M{x},-400 V500" stroke="#C9FFFA" stroke-width="{sw * 2}"/>'
                   f'</g><path d="{mp}" fill="none" stroke="#C9FFFA" stroke-width="{sw}"/></g>')
    # The spine where the two panes meet, with a whisper of tail.
    sp = 13 if not small else 22
    out.append(f'<rect x="{-sp}" y="-132" width="{2 * sp}" height="300" rx="{sp}" fill="url(#spine)"/>')
    out.append(f'<rect x="{-sp}" y="-132" width="{2 * sp}" height="300" rx="{sp}" fill="#7FF6FF" filter="url(#glow8)" opacity="0.7"/>')
    if not small:
        out.append('<path d="M0,170 C4,230 -6,290 8,350" stroke="#9FF3FA" stroke-width="9" stroke-linecap="round" fill="none"/>')
    out.append('</g></g>')
    return svg(defs, "\n".join(out))


# --------------------------------------------------------- concept c: Monogram


def m_manta():
    """An "M" that is a manta from above: the cephalic fins are the two peaks, the mouth the
    notch, the swept wings the outer stems (their tips flare out like serifs) and the
    counters the curve of the trailing edge. Kept letter-like so it never reads as a bat."""
    notch = (512, 404)
    segs = [
        ((528, 378), (546, 300), (554, 268)),     # inner side of the right cephalic fin
        ((562, 236), (608, 234), (614, 266)),     # rounded paddle tip (the peak of the M)
        ((618, 296), (632, 316), (666, 320)),     # fin to the wing's leading edge
        ((768, 328), (818, 374), (822, 460)),     # rounded shoulder
        ((826, 560), (828, 680), (858, 764)),     # outer stem, flaring into the wing tip
        ((866, 786), (838, 796), (824, 780)),     # tip
        ((774, 712), (734, 620), (726, 536)),     # inner side of the stem (trailing edge)
        ((720, 480), (700, 456), (672, 456)),     # top of the counter
        ((636, 456), (604, 488), (590, 548)),     # back down to the body
        ((580, 592), (556, 636), (520, 664)),     # rear body, tapering into the tail
    ]
    d = [f"M{_fmt(notch)}"]
    for c1, c2, e in segs:
        d.append(f"C{_fmt(c1)} {_fmt(c2)} {_fmt(e)}")
    pts = [notch] + [e for _, _, e in segs]
    last = segs[-1][2]
    d.append(f"L{_fmt((1024 - last[0], last[1]))}")
    for i in range(len(segs) - 1, -1, -1):
        c1, c2, e = segs[i]
        p0 = pts[i]
        d.append(f"C{_fmt((1024 - c2[0], c2[1]))} {_fmt((1024 - c1[0], c1[1]))} {_fmt((1024 - p0[0], p0[1]))}")
    return " ".join(d) + " Z"


def concept_c(small):
    """A bold M-shaped manta in white on a sunset gradient; the tail is a flowing swash."""
    defs = (lin("bg", [(0, "#FFC46B"), (0.45, "#FF7A63"), (1, "#D63E8C")], 0.2, 0, 0.8, 1)
            + lin("m", [(0, "#FFFFFF"), (1, "#FFEDEF")], 0, 0, 0, 1))
    out = [body("url(#bg)")]
    tr = "translate(512 512) scale(1.0) translate(-512 -512)" if not small else "translate(512 500) scale(1.12) translate(-512 -512)"
    out.append(f'<g transform="{tr}"><g filter="url(#lift)">')
    out.append(f'<path d="{m_manta()}" fill="url(#m)"/>')
    tail = ('M502,630 C498,716 526,790 590,846 C596,852 590,861 582,857 C508,808 482,730 488,636 Z' if not small else
            'M500,630 C496,700 512,750 552,796 L534,810 C484,758 472,700 478,636 Z')
    out.append(f'<path d="{tail}" fill="url(#m)"/>')
    out.append('</g></g>')
    return svg(defs, "\n".join(out))


# ---------------------------------------------------------- concept d: Surface


def caustics(seed=7, n=6, jitter=0.36, lo=60, hi=964):
    """Voronoi cells of a jittered grid, as rounded polygons: the caustic light network."""
    rnd = random.Random(seed)
    step = (hi - lo) / n
    pts = [(lo + (i + 0.5 + rnd.uniform(-jitter, jitter)) * step, lo + (j + 0.5 + rnd.uniform(-jitter, jitter)) * step)
           for i in range(-1, n + 1) for j in range(-1, n + 1)]
    cells = []
    for p in pts:
        poly = [(lo - 200, lo - 200), (hi + 200, lo - 200), (hi + 200, hi + 200), (lo - 200, hi + 200)]
        for q in pts:
            if q is p:
                continue
            mx, my = (p[0] + q[0]) / 2, (p[1] + q[1]) / 2
            nx, ny = q[0] - p[0], q[1] - p[1]
            new = []
            for k in range(len(poly)):
                a, b = poly[k], poly[(k + 1) % len(poly)]
                da = (a[0] - mx) * nx + (a[1] - my) * ny
                db = (b[0] - mx) * nx + (b[1] - my) * ny
                if da <= 0:
                    new.append(a)
                if (da < 0) != (db < 0):
                    t = da / (da - db)
                    new.append((a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t))
            poly = new
            if not poly:
                break
        if len(poly) >= 3:
            cells.append(poly)
    return cells


def concept_d(small):
    """A manta just under a sunlit, translucent water surface with caustic light."""
    defs = (lin("bg", [(0, "#8AF3DD"), (0.5, "#34C8C0"), (1, "#0F8E9E")], 0.2, 0, 0.8, 1)
            + lin("ray", [(0, "#063A48"), (1, "#0A5664")], 0, 0, 0, 1)
            + lin("fade", [(0, "#fff"), (0.5, "#fff", 0.45), (1, "#fff", 0.08)], 0, 0, 0, 1))
    out = [body("url(#bg)")]
    tr = "translate(512 492) rotate(-12) scale(0.9)" if not small else "translate(512 500) rotate(-12) scale(1.12)"
    mp = manta_path(tail=170 if not small else 60, small=small)
    out.append(f'<g clip-path="url(#body)"><g transform="{tr}">')
    out.append(f'<path d="{mp}" fill="#04303C" fill-opacity="0.22" filter="url(#glow)" transform="translate(-16 44)"/>')
    out.append(f'<path d="{mp}" fill="url(#ray)" fill-opacity="{0.86 if not small else 0.95}"/>')
    out.append('</g></g>')
    if not small:
        cells = caustics(seed=5, n=8, jitter=0.4)
        net = "".join(f'<path d="{rpoly(c, 30)}"/>' for c in cells)
        defs += ('\n  <mask id="cm" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">'
                 '<rect width="1024" height="1024" fill="url(#fade)"/>'
                 f'<path d="{mp}" transform="{tr}" fill="#000" fill-opacity="0.75"/></mask>')
        out.append(f'<g clip-path="url(#body)"><g mask="url(#cm)" fill="none" stroke="#F4FFF9" stroke-linejoin="round">'
                   f'<g stroke-width="10" stroke-opacity="0.20" filter="url(#glow8)">{net}</g>'
                   f'<g stroke-width="2.5" stroke-opacity="0.50">{net}</g></g></g>')
        # Surface sheen: a broad soft highlight across the top.
        out.append('<ellipse cx="420" cy="170" rx="360" ry="90" fill="#fff" fill-opacity="0.18" filter="url(#glow)" clip-path="url(#body)"/>')
    return svg(defs, "\n".join(out))


# ------------------------------------------------------------ wordmark

# Custom geometric lettering on a 220-unit cap height (no font needed).
CAP, XH, ASC, SW = 220, 158, 232, 38


def letters():
    """Return (paths, advance) for "Manta"; centre-line strokes, baseline y=0."""
    h = SW / 2
    out = []
    # M: the middle vertex dips to the waist, like the notch between the cephalic fins.
    x0, W = h, 196
    out.append(f"M{x0},{-h} V{-CAP + h} L{x0 + W / 2},{-CAP * 0.36} L{x0 + W},{-CAP + h} V{-h}")
    x = x0 + W + h + 30
    ro = (XH - SW) / 2

    def a(x):
        c = x + h + ro
        out.append(f"M{c - ro},{-XH / 2} A{ro},{ro} 0 1 1 {c + ro},{-XH / 2} A{ro},{ro} 0 1 1 {c - ro},{-XH / 2} Z")
        out.append(f"M{c + ro},{-XH + h} V{-h}")
        return c + ro + h
    x = a(x) + 30
    # n
    xn, rn = x + h, 52
    out.append(f"M{xn},{-h} V{-XH + h} M{xn},{-XH + h + rn} A{rn},{rn} 0 0 1 {xn + 2 * rn},{-XH + h + rn} V{-h}")
    x = xn + 2 * rn + h + 26
    # t
    xt, rt = x + h + 30, 44
    out.append(f"M{xt},{-ASC * 0.88} V{-h - rt} A{rt},{rt} 0 0 0 {xt + rt},{-h} H{xt + rt + 12}")
    out.append(f"M{xt - 40},{-XH + h} H{xt + 64}")
    x = xt + rt + 12 + h + 14
    x = a(x)
    return out, x


SET = IconSet(
    name="Manta",
    here=os.path.dirname(os.path.abspath(__file__)),
    concepts={"a": ("Glide", concept_a), "b": ("Panes", concept_b),
              "c": ("Monogram", concept_c), "d": ("Surface", concept_d)},
    lettering=Lettering(letters, cap=CAP, stroke=SW),
    recommended="a",
    ink_light="#0B2A45",
    ink_dark="#FFFFFF",
    dmg_bg=("#F7FCFD", "#E2F1F5"),
    dmg_accent="#1592B0",
    dmg_caption="Ziehe Manta in Programme · Drag Manta to Applications",
    dmg_caption_ink="#4C6B78",
)

if __name__ == "__main__":
    main(SET)
