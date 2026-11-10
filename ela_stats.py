from PIL import Image, ImageChops, ImageEnhance
import os

# Analyze the ELA image - check if any regions stand out (manipulation indicators)
ela = Image.open("/home/ritche/nanomite/osin_ela.png").convert("L")
w, h = ela.size
print("ELA size:", ela.size)

# Divide into grid and check max brightness per cell
grid = 8
cell_w, cell_h = w // grid, h // grid
max_vals = []
for gy in range(grid):
    row = []
    for gx in range(grid):
        box = (gx*cell_w, gy*cell_h, (gx+1)*cell_w, (gy+1)*cell_h)
        cell = ela.crop(box)
        extrema = cell.getextrema()
        row.append(extrema[1])
    max_vals.append(row)

print("ELA max brightness grid (8x8):")
for row in max_vals:
    print(" ".join(f"{v:3d}" for v in row))

overall_max = ela.getextrema()[1]
print("Overall ELA max:", overall_max)
print("ELA mean (approx):", sum(sum(r) for r in max_vals)/64 / 8.0 if max_vals else 0)
