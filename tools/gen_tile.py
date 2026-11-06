#!/usr/bin/env python3
"""Draw the catalog tile (docs/tile.svg, docs/tile.png): the device running a plugin, and the same plugin's panel
in the desktop app beside a list of the plugins MPC loaded and MPC's transport, a knob turned on one moving on the
other. Generated from a seed, so a rerun gives the same picture.

  python3 tools/gen_tile.py [--seed N]       needs chromium (or CHROMIUM=...) for the PNG
"""
import argparse
import math
import os
import random
import shutil
import subprocess
import tempfile

W, H = 1200, 600                  # the catalog card shows its picture at 2:1
BG, INK, DIM, BODY, EDGE = "#1b1c1f", "#f4f4f4", "#3a3c42", "#2b2d33", "#44474e"
ACCENT, AMBER, TEAL = "#d63a30", "#e0a100", "#3f8f8a"
PALETTE = [ACCENT, AMBER, TEAL, "#5a6bd0", "#9b4fb0", "#c46a2a"]


def knob(cx, cy, r, value, colour):
    """A knob: a track arc over 270 degrees, the value part lit, and a pointer."""
    a0, a1 = math.radians(135), math.radians(135 + 270 * value)

    def pt(a, rr):
        return cx + rr * math.cos(a), cy + rr * math.sin(a)

    def arc(a_from, a_to, rr):
        x0, y0 = pt(a_from, rr)
        x1, y1 = pt(a_to, rr)
        large = 1 if a_to - a_from > math.pi else 0
        return "M %.1f %.1f A %.1f %.1f 0 %d 1 %.1f %.1f" % (x0, y0, rr, rr, large, x1, y1)

    o = ['<path d="%s" fill="none" stroke="%s" stroke-width="%g" stroke-linecap="round"/>'
         % (arc(a0, math.radians(405), r * 1.25), DIM, r * 0.18)]
    if value > 0.01:
        o.append('<path d="%s" fill="none" stroke="%s" stroke-width="%g" stroke-linecap="round"/>'
                 % (arc(a0, a1, r * 1.25), colour, r * 0.18))
    o.append('<circle cx="%g" cy="%g" r="%g" fill="#16171a" stroke="%s" stroke-width="1.5"/>' % (cx, cy, r, EDGE))
    x, y = pt(a1, r * 0.75)
    o.append('<line x1="%g" y1="%g" x2="%.1f" y2="%.1f" stroke="%s" stroke-width="%g" stroke-linecap="round"/>'
             % (cx, cy, x, y, INK, max(2, r * 0.12)))
    return "\n".join(o)


def panel(seed, x, y, w, h, scale):
    """A made-up plugin panel: a title bar, two rows of knobs, a meter. Same seed, same values."""
    rng = random.Random(seed)
    o = ['<rect x="%g" y="%g" width="%g" height="%g" fill="#0e0f11"/>' % (x, y, w, h)]
    hh = h * 0.13
    o.append('<rect x="%g" y="%g" width="%g" height="%g" fill="#26282d"/>' % (x, y, w, hh))
    o.append('<rect x="%g" y="%g" width="%g" height="%g" rx="2" fill="%s"/>'
             % (x + w * 0.05, y + hh * 0.3, w * 0.28, hh * 0.4, AMBER))
    o.append('<circle cx="%g" cy="%g" r="%g" fill="%s"/>' % (x + w * 0.93, y + hh / 2, hh * 0.2, TEAL))
    cols, rows = 4, 2
    kx, ky, kw, kh = x + w * 0.05, y + hh + h * 0.06, w * 0.72, h * 0.72
    r = min(kw / cols, kh / rows) * 0.27
    values = [rng.uniform(0.15, 0.9) for _ in range(cols * rows)]
    values[1] = 0.72                      # the knob being turned, the same on both
    for i, v in enumerate(values):
        c, rr = i % cols, i // cols
        cx = kx + kw * (c + 0.5) / cols
        cy = ky + kh * (rr + 0.42) / rows
        o.append(knob(cx, cy, r, v, AMBER if i == 1 else PALETTE[(i + 2) % len(PALETTE)]))
        o.append('<rect x="%g" y="%g" width="%g" height="%g" rx="1.5" fill="%s"/>'
                 % (cx - r * 0.9, cy + r * 1.7, r * 1.8, max(3, r * 0.22), DIM))
    mx, mw = x + w * 0.83, w * 0.04
    for j, lvl in enumerate((0.72, 0.6)):
        bx = mx + j * mw * 1.6
        o.append('<rect x="%g" y="%g" width="%g" height="%g" fill="%s"/>' % (bx, ky, mw, kh * 0.9, DIM))
        o.append('<rect x="%g" y="%g" width="%g" height="%g" fill="%s"/>'
                 % (bx, ky + kh * 0.9 * (1 - lvl), mw, kh * 0.9 * lvl, TEAL))
    return "\n".join(o)


def tile(seed):
    rng = random.Random(seed)
    o = ['<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 %d %d">' % (W, H, W, H),
         '<rect width="%d" height="%d" fill="%s"/>' % (W, H, BG)]
    for gx in range(12, W, 24):          # faint dot grid
        for gy in range(12, H, 24):
            o.append('<circle cx="%d" cy="%d" r="1" fill="#2a2c31"/>' % (gx, gy))

    # the device: a screen with the plugin, pads and a row of buttons
    dx, dy, dw, dh = 82, 150, 316, 198
    o.append('<rect x="%g" y="%g" width="%g" height="%g" rx="14" fill="%s" stroke="%s" stroke-width="2"/>'
             % (dx - 22, dy - 22, dw + 44, dh + 160, BODY, EDGE))
    o.append(panel(seed, dx, dy, dw, dh, 0.6))
    for i in range(8):                    # two rows of pads
        c, r = i % 4, i // 4
        lit = i in (1, 6)
        o.append('<rect x="%g" y="%g" width="44" height="44" rx="4" fill="%s"/>'
                 % (dx + c * 54, dy + dh + 22 + r * 54, PALETTE[i % 3] if lit else EDGE))
    for i in range(2):
        o.append('<rect x="%g" y="%g" width="72" height="18" rx="3" fill="%s"/>'
                 % (dx + dw - 76, dy + dh + 34 + i * 40, ACCENT if i == 0 else EDGE))

    # the desktop app: a window with the plugin list, the same panel larger, and the transport
    ax, ay, aw, ah = 610, 92, 530, 400
    o.append('<rect x="%g" y="%g" width="%g" height="%g" rx="10" fill="%s" stroke="%s" stroke-width="2"/>'
             % (ax, ay, aw, ah, BODY, EDGE))
    for i, c in enumerate((ACCENT, AMBER, TEAL)):
        o.append('<circle cx="%g" cy="%g" r="6" fill="%s"/>' % (ax + 20 + i * 20, ay + 20, c))
    o.append('<rect x="%g" y="%g" width="140" height="12" rx="6" fill="%s"/>' % (ax + 90, ay + 14, DIM))
    sx, sy, sw = ax + 12, ay + 42, 112             # the list of plugins MPC loaded
    for i in range(6):
        sel = i == 1
        o.append('<rect x="%g" y="%g" width="%g" height="34" rx="4" fill="%s"/>'
                 % (sx, sy + i * 42, sw, "#3a3420" if sel else "#232428"))
        o.append('<rect x="%g" y="%g" width="5" height="20" rx="2" fill="%s"/>'
                 % (sx + 8, sy + i * 42 + 7, PALETTE[i % len(PALETTE)]))
        o.append('<rect x="%g" y="%g" width="%g" height="8" rx="4" fill="%s"/>'
                 % (sx + 22, sy + i * 42 + 13, rng.uniform(50, 78), AMBER if sel else DIM))
    px, py = sx + sw + 12, sy
    pw = ax + aw - 12 - px
    ph = pw * dh / dw
    o.append(panel(seed, px, py, pw, ph, 1))
    ty = ay + ah - 64                              # the transport: play, stop, record, tempo
    o.append('<rect x="%g" y="%g" width="%g" height="52" rx="6" fill="#16171a"/>' % (px, ty, pw))
    o.append('<path d="M %g %g l 18 11 l -18 11 z" fill="%s"/>' % (px + 22, ty + 15, TEAL))
    o.append('<rect x="%g" y="%g" width="20" height="20" rx="2" fill="%s"/>' % (px + 62, ty + 16, INK))
    o.append('<circle cx="%g" cy="%g" r="10" fill="%s"/>' % (px + 112, ty + 26, ACCENT))
    o.append('<text x="%g" y="%g" font-family="DejaVu Sans Mono, monospace" font-size="20" fill="%s">120.00</text>'
             % (px + pw - 160, ty + 33, AMBER))
    for i in range(4):
        o.append('<rect x="%g" y="%g" width="6" height="22" rx="2" fill="%s"/>'
                 % (px + pw - 52 + i * 10, ty + 15, AMBER if i == 0 else DIM))

    # the link: knob values both ways, as two arcs of dots, and arrowheads
    x0, x1 = dx + dw + 40, ax - 18
    yc = dy + dh * 0.5
    for sign, colour, rev in ((-1, AMBER, False), (1, TEAL, True)):
        n = 26
        for k in range(n):
            t = k / (n - 1)
            x = x0 + (x1 - x0) * t
            y = yc + sign * 70 * math.sin(math.pi * t)
            s = 3 + 5 * (t if not rev else 1 - t)
            o.append('<circle cx="%.1f" cy="%.1f" r="%.1f" fill="%s" opacity="%.2f"/>'
                     % (x, y, s / 2, colour, 0.35 + 0.6 * (t if not rev else 1 - t)))
        if not rev:
            o.append('<path d="M %g %g l -14 -8 l 0 16 z" fill="%s"/>' % (x1 + 8, yc, colour))
        else:
            o.append('<path d="M %g %g l 14 -8 l 0 16 z" fill="%s"/>' % (x0 - 8, yc, colour))
    o.append("</svg>")
    return "\n".join(o)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=6730)
    a = ap.parse_args()
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs")
    os.makedirs(root, exist_ok=True)
    svg = os.path.join(root, "tile.svg")
    with open(svg, "w") as f:
        f.write(tile(a.seed))
    chrome = os.environ.get("CHROMIUM") or shutil.which("chromium") or shutil.which("chromium-browser")
    if not chrome:
        print("wrote %s (no chromium for the PNG)" % svg)
        return
    with tempfile.TemporaryDirectory() as tmp:
        page = os.path.join(tmp, "t.html")
        with open(page, "w") as f:
            f.write('<html><body style="margin:0">%s</body></html>' % open(svg).read())
        png = os.path.abspath(os.path.join(root, "tile.png"))
        subprocess.run([chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--user-data-dir=" + tmp,
                        "--window-size=%d,%d" % (W, H), "--screenshot=" + png, "file://" + page],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print("wrote %s and %s" % (svg, png))


if __name__ == "__main__":
    main()
