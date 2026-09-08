"""
Universal Downloader — application icon generator.

Mark concept: a six-blade aperture. It says "capture, from any source" without
falling back on the download-arrow cliche, stays purely geometric, and still
resolves into a recognisable silhouette at 16px.

Run:  python scripts/generate_icon.py
Out:  src-tauri/icons/icon-source.png       1024x1024 app tile
      src-tauri/icons/icon-source-flat.png  1024x1024 bare mark, transparent
"""
from __future__ import annotations

import math
import os

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
SS = 4  # supersample factor; downsampled with LANCZOS for clean edges
S = SIZE * SS

ACCENT_A = (255, 122, 61)    # --accent, dark theme
ACCENT_B = (255, 144, 89)     # --accent-hover, dark theme
TILE_TOP = (38, 33, 25)
TILE_BOTTOM = (12, 11, 9)

BLADES = 6


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vertical_gradient(size, top, bottom):
    grad = Image.new("RGB", (1, size))
    px = grad.load()
    for y in range(size):
        px[0, y] = lerp(top, bottom, y / max(size - 1, 1))
    return grad.resize((size, size), Image.Resampling.BILINEAR)


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
    inner_r = size * 0.285
    seam_w = size * 0.062

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


def build_icon(with_tile: bool) -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    if with_tile:
        radius = int(S * 0.2237)
        tile_mask = Image.new("L", (S, S), 0)
        ImageDraw.Draw(tile_mask).rounded_rectangle(
            (0, 0, S - 1, S - 1), radius=radius, fill=255
        )
        img.paste(vertical_gradient(S, TILE_TOP, TILE_BOTTOM).convert("RGBA"), (0, 0), tile_mask)

        # Light source at the top-left, clipped to the tile.
        glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        gr = S * 0.46
        ImageDraw.Draw(glow).ellipse(
            (S * 0.30 - gr, S * 0.24 - gr, S * 0.30 + gr, S * 0.24 + gr),
            fill=(99, 102, 241, 46),
        )
        glow = glow.filter(ImageFilter.GaussianBlur(S * 0.11))
        img.alpha_composite(
            Image.composite(glow, Image.new("RGBA", (S, S), (0, 0, 0, 0)), tile_mask)
        )

        mark = build_mark(int(S * 0.545))
        off = (S - mark.width) // 2
        img.alpha_composite(mark, (off, off))

        # Hairline rim so the tile keeps an edge on light backgrounds.
        rim = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        ImageDraw.Draw(rim).rounded_rectangle(
            (0, 0, S - 1, S - 1), radius=radius, outline=(255, 255, 255, 26), width=SS * 2
        )
        img.alpha_composite(rim)
    else:
        mark = build_mark(int(S * 0.92))
        off = (S - mark.width) // 2
        img.alpha_composite(mark, (off, off))

    return img.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def main():
    out_dir = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
    os.makedirs(out_dir, exist_ok=True)
    build_icon(with_tile=True).save(os.path.join(out_dir, "icon-source.png"))
    build_icon(with_tile=False).save(os.path.join(out_dir, "icon-source-flat.png"))

    # Small-size legibility check.
    src = Image.open(os.path.join(out_dir, "icon-source.png"))
    strip = Image.new("RGBA", (16 + 32 + 48 + 64 + 40, 64), (18, 18, 26, 255))
    x = 0
    for s in (16, 32, 48, 64):
        strip.alpha_composite(src.resize((s, s), Image.Resampling.LANCZOS), (x, (64 - s) // 2))
        x += s + 10
    strip.resize((strip.width * 4, strip.height * 4), Image.Resampling.NEAREST).save(
        os.path.join(out_dir, "..", "..", "scripts", "icon-preview.png")
    )
    print("wrote icon-source.png, icon-source-flat.png, scripts/icon-preview.png")


if __name__ == "__main__":
    main()

# Tray icons are produced by scripts/generate_tray.py, which reuses build_icon()
# from this module. Run both after changing the mark:
#   python scripts/generate_icon.py && python scripts/generate_tray.py
