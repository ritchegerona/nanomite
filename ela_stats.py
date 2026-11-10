#!/usr/bin/env python3
"""ELA grid statistics: per-region max brightness of an ELA map.

usage: ela_stats.py <image> [output_dir]

Reads <output_dir>/ela.png produced by ela_analysis.py (<output_dir> defaults
to the image's own folder) and reports an 8x8 max-brightness grid — regions
that stand out are manipulation indicators.
"""
import math
import os
import sys

from PIL import Image


def main():
    if len(sys.argv) < 2:
        print("usage: ela_stats.py <image> [output_dir]", file=sys.stderr)
        return 2
    out_dir = sys.argv[2] if len(sys.argv) > 2 else (os.path.dirname(sys.argv[1]) or ".")
    ela_path = os.path.join(out_dir, "ela.png")
    try:
        ela = Image.open(ela_path).convert("L")
    except Exception as e:
        print(f"ela_stats: cannot read ELA map {ela_path} (run ela_analysis.py first): {e}", file=sys.stderr)
        return 1

    w, h = ela.size
    print("ELA size:", ela.size)

    # Divide into grid and check max brightness per cell
    grid = 8
    cell_w, cell_h = w // grid, h // grid
    max_vals = []
    for gy in range(grid):
        row = []
        for gx in range(grid):
            box = (gx * cell_w, gy * cell_h, (gx + 1) * cell_w, (gy + 1) * cell_h)
            cell = ela.crop(box)
            extrema = cell.getextrema()
            row.append(extrema[1])
        max_vals.append(row)

    print("ELA max brightness grid (8x8):")
    for row in max_vals:
        print(" ".join(f"{v:3d}" for v in row))

    overall_max = ela.getextrema()[1]
    print("Overall ELA max:", overall_max)
    print("ELA mean (approx):", sum(sum(r) for r in max_vals) / 64 / 8.0 if max_vals else 0)
    return 0


if __name__ == "__main__":
    sys.exit(main())
