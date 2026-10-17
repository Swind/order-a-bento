#!/usr/bin/env python3
"""Render eval/cases/*/source.html into image.png / image.jpg (headless Chrome + Pillow).

Usage: python3 eval/render.py [case-dir-substring]
Env:   CHROME_PATH (default /usr/bin/google-chrome)

The synthetic images are reproducible from source.html; 02-roster-photo gets
photo-like degradation (rotation, low contrast, noise, blur, vignette) and is
saved as a JPEG. Everything else stays a crisp PNG (JPEG when > 300 KB).
"""
import os
import random
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageEnhance, ImageFilter, ImageDraw

ROOT = Path(__file__).resolve().parent / "cases"
CHROME = os.environ.get("CHROME_PATH", "/usr/bin/google-chrome")
WIDTH = 1200
PHOTO_CASES = {"02-roster-photo"}


def screenshot(html: Path, height: int, out: Path) -> None:
    subprocess.run(
        [
            CHROME, "--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars",
            "--force-device-scale-factor=1", f"--window-size={WIDTH},{height}",
            f"--screenshot={out}", html.resolve().as_uri(),
        ],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )


def photo_effect(img: Image.Image) -> Image.Image:
    rnd = random.Random(7)  # deterministic
    img = img.convert("RGB")
    # a dark desk around a slightly rotated sheet
    canvas = Image.new("RGB", (img.width + 160, img.height + 160), (70, 62, 55))
    canvas.paste(img, (80, 80))
    canvas = canvas.rotate(2.5, resample=Image.BICUBIC, fillcolor=(70, 62, 55))
    canvas = canvas.filter(ImageFilter.GaussianBlur(1.1))
    canvas = ImageEnhance.Contrast(canvas).enhance(0.72)
    canvas = ImageEnhance.Brightness(canvas).enhance(0.93)
    # vignette + uneven light
    w, h = canvas.size
    shade = Image.new("L", (w, h), 0)
    ImageDraw.Draw(shade).ellipse((-w * 0.2, -h * 0.2, w * 1.2, h * 1.2), fill=255)
    shade = shade.filter(ImageFilter.GaussianBlur(120))
    dark = Image.new("RGB", (w, h), (20, 18, 15))
    canvas = Image.composite(canvas, dark, shade.point(lambda v: 150 + v * 105 // 255))
    # sensor noise
    px = canvas.load()
    for y in range(h):
        for x in range(w):
            n = rnd.randint(-14, 14)
            r, g, b = px[x, y]
            px[x, y] = (max(0, min(255, r + n)), max(0, min(255, g + n)), max(0, min(255, b + n)))
    return canvas


def render(case: Path) -> None:
    html = case / "source.html"
    m = re.search(r'name="render-height" content="(\d+)"', html.read_text(encoding="utf-8"))
    height = int(m.group(1)) if m else 900
    for old in ("image.png", "image.jpg"):
        (case / old).unlink(missing_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        raw = Path(tmp) / "raw.png"
        screenshot(html, height, raw)
        img = Image.open(raw)
        if case.name in PHOTO_CASES:
            photo_effect(img).save(case / "image.jpg", quality=72, optimize=True)
        else:
            raw_png = case / "image.png"
            img.convert("RGB").save(raw_png, optimize=True)
            if raw_png.stat().st_size > 300_000:
                raw_png.unlink()
                img.convert("RGB").save(case / "image.jpg", quality=80, optimize=True)
    out = next(p for p in (case / "image.png", case / "image.jpg") if p.exists())
    print(f"{case.name}: {out.name} {out.stat().st_size // 1024} KB")


if __name__ == "__main__":
    flt = sys.argv[1] if len(sys.argv) > 1 else ""
    for d in sorted(ROOT.iterdir()):
        if d.is_dir() and (d / "source.html").exists() and flt in d.name:
            render(d)
