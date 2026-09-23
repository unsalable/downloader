"""
Universal Downloader — application icon generator.

Mark concept: a six-blade aperture. It says "capture, from any source" without
falling back on the download-arrow cliche, stays purely geometric, and still
resolves into a recognisable silhouette at 16px.

The mark is drawn flat on transparency and nothing else. It used to come in a
second flavour, sunk into a dark rounded-square tile, and that tile is gone:
the mark now has to carry itself in a Windows title bar at 16px and in the
notification area at 20px, where there is no tile to hold its shape. That is
what set the proportions below — seams narrower than a pixel at 16px read as a
grey wash rather than a cut, which turns the blades back into a plain ring.

The same proportions are written twice more — as vectors in
src/components/layout/Logo.tsx, and as a pixel test in
extension/make-icons.mjs. Change one and you have to change all three.

There is a second output, and it is the same mark with more air around it.
Android composes a launcher icon from a foreground drawn on a 108dp canvas of
which only the middle 72dp is guaranteed to survive the launcher's mask, so a
foreground that reaches the edge loses its rim to whatever shape the phone
crops to. The Tauri CLI has an `android_fg_scale` for exactly this and it does
nothing in the version this repository uses, measured rather than assumed, so
the inset is drawn in instead.

Run:  python scripts/generate_icon.py
Out:  src-tauri/icons/icon-source-flat.png  1024x1024 bare mark, transparent
      src-tauri/icons/icon-source-fg.png    the same, inset for Android
"""
from __future__ import annotations

import math
import os

from PIL import Image, ImageDraw

SIZE = 1024
SS = 4  # supersample factor; downsampled with LANCZOS for clean edges
S = SIZE * SS

ACCENT_A = (255, 122, 61)    # --accent, dark theme
ACCENT_B = (255, 144, 89)     # --accent-hover, dark theme

BLADES = 6

# Both as a fraction of the disc's diameter, so the shape is resolution-free.
INNER_R = 0.335
SEAM_W = 0.085

# The disc's share of the square it is drawn into. The margin is what keeps the
# rim off the edge of a title-bar icon, which otherwise looks wedged in.
MARK_FILL = 0.92

# The same share for the Android adaptive foreground, chosen to sit inside the
# 72dp of a 108dp canvas that a launcher mask cannot crop.
ANDROID_FILL = 0.60


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def diagonal_gradient(size, a, b):
    n = 96
    grad = Image.new("RGB", (n, n))
    px = grad.load()
    for y in range(n):
        for x in range(n):
            t = (x / (n - 1) * 0.65 + y / (n - 1) * 0.35)
            px[x, y] = lerp(a, b, t)
    return grad.resize((size, size), Image.Resampling.BICUBIC)


def _poly(cx, cy, radius, n, rotation_deg):
    return [
        (
            cx + radius * math.cos(math.radians(rotation_deg + i * 360 / n)),
            cy + radius * math.sin(math.radians(rotation_deg + i * 360 / n)),
        )
        for i in range(n)
    ]


def aperture_mask(size: int) -> Image.Image:
    """Filled disc minus a central polygon opening minus the blade seams."""
    c = size / 2
    outer_r = size * 0.5
    inner_r = size * INNER_R
    seam_w = size * SEAM_W

    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    d.ellipse((c - outer_r, c - outer_r, c + outer_r, c + outer_r), fill=255)

    rotation = -90  # flat blade facing up
    opening = _poly(c, c, inner_r, BLADES, rotation)
    d.polygon(opening, fill=0)

    # Each seam runs from an opening vertex along that edge's direction and out
    # past the rim. The tangential (rather than radial) direction is what gives
    # an iris its characteristic swirl.
    for i in range(BLADES):
        x0, y0 = opening[i]
        x1, y1 = opening[(i + 1) % BLADES]
        dx, dy = x1 - x0, y1 - y0
        length = math.hypot(dx, dy)
        ux, uy = dx / length, dy / length
        nx, ny = -uy * seam_w / 2, ux * seam_w / 2
        far = size  # comfortably past the rim
        quad = [
            (x0 + nx, y0 + ny),
            (x0 - nx, y0 - ny),
            (x0 + ux * far - nx, y0 + uy * far - ny),
            (x0 + ux * far + nx, y0 + uy * far + ny),
        ]
        d.polygon(quad, fill=0)
        # Round off the seam's inner end so blades do not end in a hard spike.
        r = seam_w / 2
        d.ellipse((x0 - r, y0 - r, x0 + r, y0 + r), fill=0)

    return mask


def build_mark(size: int) -> Image.Image:
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    paint = diagonal_gradient(size, ACCENT_A, ACCENT_B).convert("RGBA")
    layer.paste(paint, (0, 0), aperture_mask(size))
    return layer


def build_icon(fill: float = MARK_FILL) -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    mark = build_mark(int(S * fill))
    off = (S - mark.width) // 2
    img.alpha_composite(mark, (off, off))
    return img.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def write_preview(src: Image.Image, path: str):
    """A strip at the sizes that actually decide the shape, on both grounds.

    Over one ground only it is easy to believe the mark still reads when what
    the eye is really following is the contrast, so the same row is laid over
    the dark title bar and a light one.
    """
    sizes = (16, 20, 24, 32, 48)
    zoom, pad = 6, 8
    row = 48 * zoom + pad * 2
    width = pad + sum(s * zoom + pad for s in sizes)

    strip = Image.new("RGB", (width, row * 2))
    for index, ground in enumerate(((0x20, 0x20, 0x20), (0xF3, 0xF3, 0xF3))):
        band = Image.new("RGB", (width, row), ground)
        x = pad
        for s in sizes:
            tile = src.resize((s, s), Image.Resampling.LANCZOS)
            big = tile.resize((s * zoom, s * zoom), Image.Resampling.NEAREST)
            under = Image.new("RGBA", big.size, ground + (255,))
            under.alpha_composite(big)
            band.paste(under.convert("RGB"), (x, (row - s * zoom) // 2))
            x += s * zoom + pad
        strip.paste(band, (0, row * index))
    strip.save(path)


def main():
    here = os.path.dirname(__file__)
    out_dir = os.path.join(here, "..", "src-tauri", "icons")
    os.makedirs(out_dir, exist_ok=True)

    icon = build_icon()
    icon.save(os.path.join(out_dir, "icon-source-flat.png"))
    build_icon(ANDROID_FILL).save(os.path.join(out_dir, "icon-source-fg.png"))
    write_preview(icon, os.path.join(here, "icon-preview.png"))
    print("wrote icon-source-flat.png, icon-source-fg.png, scripts/icon-preview.png")


if __name__ == "__main__":
    main()
