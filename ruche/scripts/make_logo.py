#!/usr/bin/env python3
"""Generate the provisional Ruche logo and write it over the DBFlux branding assets.

The DBFlux trademark policy requires modified builds to ship their own name and
logo. This script draws three honeycomb cells and writes every file the app and
the packaging read: the stable marks, the monochrome glyph, the per-size PNGs,
the macOS .icns and the Windows .ico. Nightly assets are left untouched.

Usage (from the repository root):
    python3 ruche/scripts/make_logo.py

Requires Pillow and, for the .icns, macOS `iconutil`.
"""

import math
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
YELLOW = "#fad83b"
DARK = "#181818"
VIEWBOX = 64
CELL_RADIUS = 12.0
CELL_GAP = 2.5
CORNER_RADIUS = 14


def hexagon(cx: float, cy: float, r: float) -> list[tuple[float, float]]:
    """Pointy-top hexagon around (cx, cy)."""
    return [
        (cx + r * math.cos(math.radians(-90 + 60 * k)), cy + r * math.sin(math.radians(-90 + 60 * k)))
        for k in range(6)
    ]


def cells() -> list[list[tuple[float, float]]]:
    """Three cells: one on top, two below, centred in the 64×64 view box."""
    width = math.sqrt(3) * CELL_RADIUS
    dx = (width + CELL_GAP) / 2
    dy = 1.5 * CELL_RADIUS + CELL_GAP * math.sqrt(3) / 2
    top_y = VIEWBOX / 2 - dy / 2
    return [
        hexagon(VIEWBOX / 2, top_y, CELL_RADIUS),
        hexagon(VIEWBOX / 2 - dx, top_y + dy, CELL_RADIUS),
        hexagon(VIEWBOX / 2 + dx, top_y + dy, CELL_RADIUS),
    ]


def svg_path() -> str:
    parts = []
    for points in cells():
        head, *tail = points
        parts.append(f"M{head[0]:.2f} {head[1]:.2f}" + "".join(f"L{x:.2f} {y:.2f}" for x, y in tail) + "Z")
    return " ".join(parts)


def mark_svg() -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {VIEWBOX} {VIEWBOX}"><title>Ruche</title>'
        f'<rect width="{VIEWBOX}" height="{VIEWBOX}" rx="{CORNER_RADIUS}" fill="{DARK}"></rect>'
        f'<path d="{svg_path()}" fill="{YELLOW}"></path></svg>\n'
    )


def glyph_svg() -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {VIEWBOX} {VIEWBOX}" width="{VIEWBOX}" '
        f'height="{VIEWBOX}" role="img" aria-label="Ruche"><path d="{svg_path()}" fill="currentColor"></path></svg>\n'
    )


def wordmark_svg() -> str:
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 240 64"><title>Ruche</title>'
        f'<rect width="{VIEWBOX}" height="{VIEWBOX}" rx="{CORNER_RADIUS}" fill="{DARK}"></rect>'
        f'<path d="{svg_path()}" fill="{YELLOW}"></path>'
        '<text x="80" y="44" font-family="Roboto, Helvetica, Arial, sans-serif" font-size="36" '
        'font-weight="700" fill="currentColor">Ruche</text></svg>\n'
    )


def render_png(size: int, background: bool = True) -> Image.Image:
    """Draw at 4× and downscale for anti-aliased edges."""
    scale = size * 4 / VIEWBOX
    image = Image.new("RGBA", (size * 4, size * 4), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    if background:
        draw.rounded_rectangle(
            (0, 0, size * 4 - 1, size * 4 - 1), radius=int(CORNER_RADIUS * scale), fill=DARK
        )
    for points in cells():
        draw.polygon([(x * scale, y * scale) for x, y in points], fill=YELLOW)
    return image.resize((size, size), Image.LANCZOS)


def write_icns(target: Path) -> None:
    if shutil.which("iconutil") is None:
        print("iconutil not found: skipping the .icns", file=sys.stderr)
        return
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "ruche.iconset"
        iconset.mkdir()
        for base in (16, 32, 128, 256, 512):
            render_png(base).save(iconset / f"icon_{base}x{base}.png")
            render_png(base * 2).save(iconset / f"icon_{base}x{base}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(target)], check=True)


def main() -> None:
    branding = ROOT / "resources" / "branding"
    icons = ROOT / "packaging" / "icons"

    (branding / "glyph.svg").write_text(glyph_svg())
    stable = branding / "stable"
    (stable / "mark.svg").write_text(mark_svg())
    (stable / "mark-small.svg").write_text(mark_svg())
    (stable / "wordmark.svg").write_text(wordmark_svg())
    render_png(256).save(stable / "mark-256.png")
    render_png(256).save(stable / "mark-small-256.png")

    for size in (16, 32, 48, 64, 128, 256, 512):
        render_png(size).save(icons / f"{size}x{size}" / "apps" / "dbflux.png")

    render_png(256).save(icons / "dbflux.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)])
    write_icns(icons / "dbflux.icns")
    print("Ruche logo written over the stable DBFlux branding assets.")


if __name__ == "__main__":
    main()
