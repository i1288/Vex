#!/usr/bin/env python3
"""Generate the Vex logo: assets/logo.svg plus the PNG and ICO icons.

The logo is a rounded blue tile with a two-tone "V" and a block cursor,
the mark of a modal editor. Needs Pillow:  python3 scripts/make_icons.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ASSETS = Path(__file__).resolve().parent.parent / "assets"
SIZE = 1024

# Geometry in a 1024 x 1024 box (shared by the SVG and the PNGs).
TILE = (64, 64, 960, 960)
TILE_RADIUS = 200
TOP, BOTTOM = (0x23, 0x86, 0xE8), (0x0B, 0x3A, 0x7E)
V_LEFT = [(262, 286), (398, 286), (512, 594), (512, 742), (440, 742)]
V_RIGHT = [(512, 594), (626, 286), (762, 286), (584, 742), (512, 742)]
CURSOR = (688, 566, 772, 742)
CURSOR_RADIUS = 12
CURSOR_COLOR = (0x4E, 0xE6, 0xB4)
SHADOW_OFFSET = 14


def hexrgb(c):
    return "#%02x%02x%02x" % c


def svg():
    pts = lambda p: " ".join(f"{x},{y}" for x, y in p)
    x0, y0, x1, y1 = TILE
    cx0, cy0, cx1, cy1 = CURSOR
    shadow = lambda p: [(x, y + SHADOW_OFFSET) for x, y in p]
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}" width="{SIZE}" height="{SIZE}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{hexrgb(TOP)}"/>
      <stop offset="1" stop-color="{hexrgb(BOTTOM)}"/>
    </linearGradient>
  </defs>
  <rect x="{x0}" y="{y0}" width="{x1 - x0}" height="{y1 - y0}" rx="{TILE_RADIUS}" fill="url(#bg)"/>
  <polygon points="{pts(shadow(V_LEFT))} {pts(shadow(V_RIGHT))}" fill="#000" fill-opacity="0.22"/>
  <polygon points="{pts(V_LEFT)}" fill="#fff"/>
  <polygon points="{pts(V_RIGHT)}" fill="#fff" fill-opacity="0.78"/>
  <rect x="{cx0}" y="{cy0 + SHADOW_OFFSET}" width="{cx1 - cx0}" height="{cy1 - cy0}" rx="{CURSOR_RADIUS}" fill="#000" fill-opacity="0.22"/>
  <rect x="{cx0}" y="{cy0}" width="{cx1 - cx0}" height="{cy1 - cy0}" rx="{CURSOR_RADIUS}" fill="{hexrgb(CURSOR_COLOR)}"/>
</svg>
"""


def render(size):
    """Draw the logo at `size` px, supersampled 4x for smooth edges."""
    s = size * 4 / SIZE
    big = size * 4
    sc = lambda p: [(x * s, y * s) for x, y in p]
    box = lambda b: tuple(v * s for v in b)

    grad = Image.linear_gradient("L").resize((big, big))
    bg = Image.composite(Image.new("RGBA", (big, big), BOTTOM + (255,)), Image.new("RGBA", (big, big), TOP + (255,)), grad)
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(box(TILE), radius=TILE_RADIUS * s, fill=255)
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    img.paste(bg, (0, 0), mask)

    # Soft drop shadow under the mark.
    shadow = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(shadow)
    off = SHADOW_OFFSET * s
    for poly in (V_LEFT, V_RIGHT):
        d.polygon([(x, y + off) for x, y in sc(poly)], fill=(0, 0, 0, 70))
    cx0, cy0, cx1, cy1 = box(CURSOR)
    d.rounded_rectangle((cx0, cy0 + off, cx1, cy1 + off), radius=CURSOR_RADIUS * s, fill=(0, 0, 0, 70))
    shadow = shadow.filter(ImageFilter.GaussianBlur(10 * s))
    img = Image.alpha_composite(img, shadow)

    mark = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(mark)
    d.polygon(sc(V_LEFT), fill=(255, 255, 255, 255))
    d.polygon(sc(V_RIGHT), fill=(255, 255, 255, 199))
    d.rounded_rectangle(box(CURSOR), radius=CURSOR_RADIUS * s, fill=CURSOR_COLOR + (255,))
    img = Image.alpha_composite(img, mark)
    # Keep everything inside the tile (the shadow blur spreads past it).
    clip = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    clip.paste(img, (0, 0), mask)
    return clip.resize((size, size), Image.LANCZOS)


def main():
    ASSETS.mkdir(exist_ok=True)
    (ASSETS / "logo.svg").write_text(svg())
    render(1024).save(ASSETS / "icon.png")
    render(256).save(ASSETS / "icon-256.png")
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    render(256).save(ASSETS / "icon.ico", sizes=[(n, n) for n in ico_sizes])
    print("wrote", ", ".join(p.name for p in sorted(ASSETS.iterdir())))


if __name__ == "__main__":
    main()
