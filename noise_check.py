from PIL import Image
import math

img = Image.open("/home/ritche/nanomite/osin_forensic.jpg").convert("L")
w, h = img.size
px = img.load()

# Simple local variance / noise consistency check across 4 quadrants
def block_stats(x0,y0,x1,y1):
    vals = []
    for y in range(y0,y1,4):
        for x in range(x0,x1,4):
            vals.append(px[x,y])
    n = len(vals)
    mean = sum(vals)/n
    var = sum((v-mean)**2 for v in vals)/n
    return mean, math.sqrt(var)

quads = {
    "TL (face top-left)": (0,0,w//2,h//2),
    "TR (face top-right)": (w//2,0,w,h//2),
    "BL (neck bottom-left)": (0,h//2,w//2,h),
    "BR (shoulder bottom-right)": (w//2,h//2,w,h),
}
for name,(x0,y0,x1,y1) in quads.items():
    m,s = block_stats(x0,y0,x1,y1)
    print(f"{name}: mean={m:.1f} noise_std={s:.2f}")
