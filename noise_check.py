#!/usr/bin/env python3
"""Noise consistency check across image quadrants.

usage: noise_check.py <image>

Local variance per quadrant: a pasted-in region usually carries noise
statistics that differ from the rest of the image.
"""
import math
import sys

from PIL import Image


def main():
    if len(sys.argv) < 2:
        print("usage: noise_check.py <image>", file=sys.stderr)
        return 2
    img_path = sys.argv[1]
    try:
        img = Image.open(img_path).convert("L")
    except Exception as e:
        print(f"noise_check: cannot read image {img_path}: {e}", file=sys.stderr)
        return 1

    w, h = img.size
    px = img.load()

    # Simple local variance / noise consistency check across 4 quadrants
    def block_stats(x0, y0, x1, y1):
        vals = []
        for y in range(y0, y1, 4):
            for x in range(x0, x1, 4):
                vals.append(px[x, y])
        n = len(vals)
        mean = sum(vals) / n
        var = sum((v - mean) ** 2 for v in vals) / n
        return mean, math.sqrt(var)

    quads = {
        "TL (top-left)": (0, 0, w // 2, h // 2),
        "TR (top-right)": (w // 2, 0, w, h // 2),
        "BL (bottom-left)": (0, h // 2, w // 2, h),
        "BR (bottom-right)": (w // 2, h // 2, w, h),
    }
    for name, (x0, y0, x1, y1) in quads.items():
        m, s = block_stats(x0, y0, x1, y1)
        print(f"{name}: mean={m:.1f} noise_std={s:.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
