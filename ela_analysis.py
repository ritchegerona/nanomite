#!/usr/bin/env python3
"""Error Level Analysis (ELA) for JPEG manipulation detection.

usage: ela_analysis.py <image> [output_dir]

Writes <output_dir>/resaved.jpg (quality-90 resave) and <output_dir>/ela.png
(brightness-boosted pixel difference). <output_dir> defaults to the image's
own folder.
"""
import os
import sys

from PIL import Image, ImageChops, ImageEnhance


def main():
    if len(sys.argv) < 2:
        print("usage: ela_analysis.py <image> [output_dir]", file=sys.stderr)
        return 2
    img_path = sys.argv[1]
    out_dir = sys.argv[2] if len(sys.argv) > 2 else (os.path.dirname(img_path) or ".")
    try:
        img = Image.open(img_path).convert("RGB")
    except Exception as e:
        print(f"ela_analysis: cannot read image {img_path}: {e}", file=sys.stderr)
        return 1

    print("Image size:", img.size)

    # ELA: resave at quality 90, compare
    temp_path = os.path.join(out_dir, "resaved.jpg")
    img.save(temp_path, "JPEG", quality=90)
    resaved = Image.open(temp_path)
    diff = ImageChops.difference(img, resaved)
    extrema = diff.getextrema()
    max_diff = max([e[1] for e in extrema])
    print("ELA max pixel difference:", max_diff)

    # Enhance diff for visibility
    enh = ImageEnhance.Brightness(diff)
    diff_enhanced = enh.enhance(10.0)
    ela_path = os.path.join(out_dir, "ela.png")
    diff_enhanced.save(ela_path)
    print("ELA saved to", ela_path)

    # Pixel stats via histogram
    h = img.histogram()

    def mean_std(chan_offset):
        vals = h[chan_offset:chan_offset + 256]
        total = sum(vals)
        mean = sum(i * v for i, v in enumerate(vals)) / total if total else 0
        var = sum(((i - mean) ** 2) * v for i, v in enumerate(vals)) / total if total else 0
        return round(mean, 2), round(var ** 0.5, 2)

    for name, off in [("R", 0), ("G", 256), ("B", 512)]:
        m, s = mean_std(off)
        print(f"{name} mean={m} std={s}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
