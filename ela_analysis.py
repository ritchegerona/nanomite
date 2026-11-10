from PIL import Image, ImageChops, ImageEnhance

img_path = "/home/ritche/nanomite/osin_forensic.jpg"
img = Image.open(img_path).convert("RGB")
print("Image size:", img.size)

# ELA: resave at quality 90, compare
temp_path = "/home/ritche/nanomite/osin_resaved.jpg"
img.save(temp_path, "JPEG", quality=90)
resaved = Image.open(temp_path)
diff = ImageChops.difference(img, resaved)
extrema = diff.getextrema()
max_diff = max([e[1] for e in extrema])
print("ELA max pixel difference:", max_diff)

# Enhance diff for visibility
enh = ImageEnhance.Brightness(diff)
diff_enhanced = enh.enhance(10.0)
diff_enhanced.save("/home/ritche/nanomite/osin_ela.png")
print("ELA saved to osin_ela.png")

# Pixel stats via histogram
h = img.histogram()
def mean_std(chan_offset):
    vals = h[chan_offset:chan_offset+256]
    total = sum(vals)
    mean = sum(i*v for i,v in enumerate(vals))/total if total else 0
    var = sum(((i-mean)**2)*v for i,v in enumerate(vals))/total if total else 0
    return round(mean,2), round(var**0.5,2)
for name, off in [("R",0),("G",256),("B",512)]:
    m,s = mean_std(off)
    print(f"{name} mean={m} std={s}")
