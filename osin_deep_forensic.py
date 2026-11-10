#!/usr/bin/env python3
"""
Deep forensic metadata digger for a JPEG file.
Parses all JPEG markers, Exif IFDs, Photoshop 8BIM resources, and scans for hidden data.

usage: osin_deep_forensic.py <image>
"""
import struct, os, hashlib, binascii, sys

if len(sys.argv) > 1:
    PATH = sys.argv[1]
else:
    print("usage: osin_deep_forensic.py <image>", file=sys.stderr)
    sys.exit(2)
if not os.path.exists(PATH):
    print(f"osin_deep_forensic: file not found: {PATH}", file=sys.stderr)
    sys.exit(1)

with open(PATH, "rb") as f:
    data = f.read()

size = len(data)
print(f"=" * 70)
print(f"FORENSIC DEEP DIG: {PATH}")
print(f"=" * 70)
print(f"File size: {size} bytes ({size / 1024:.2f} KB)")
print(f"SHA-256:   {hashlib.sha256(data).hexdigest()}")
print(f"MD5:       {hashlib.md5(data).hexdigest()}")
print()

# ---------------------------------------------------------------------------
# 1. JPEG SEGMENT MAP
# ---------------------------------------------------------------------------
MARKERS = {
    0xD8: "SOI (Start of Image)",
    0xD9: "EOI (End of Image)",
    0xDA: "SOS (Start of Scan)",
    0xDB: "DQT (Define Quantization Table)",
    0xC0: "SOF0 (Baseline DCT)",
    0xC1: "SOF1 (Extended sequential)",
    0xC2: "SOF2 (Progressive)",
    0xC4: "DHT (Define Huffman Table)",
    0xDD: "DRI (Define Restart Interval)",
    0xFE: "COM (Comment)",
    0xE0: "APP0 (JFIF)",
    0xE1: "APP1 (Exif/XMP)",
    0xE2: "APP2 (ICC/FlashPix)",
    0xED: "APP14 (Photoshop/Adobe)",
    0xEE: "APP13 (IPTC)",
}

print("-" * 70)
print("JPEG SEGMENT MAP")
print("-" * 70)
print(f"{'Offset':<10} {'Marker':<8} {'Name':<30} {'Length':<8} {'Detail'}")
print("-" * 70)

offset = 0
segments = []
while offset < size - 1:
    b1, b2 = data[offset], data[offset + 1]
    if b1 != 0xFF:
        break
    marker = b2
    name = MARKERS.get(marker, f"UNKNOWN 0x{marker:02X}")
    
    if marker in (0xD8, 0xD9, 0x01):  # SOI, EOI, TEM - no length
        seg_len = 0
        detail = ""
        next_offset = offset + 2
    elif marker == 0xDA:  # SOS - scan data, find next FFD9 or FFxx
        # SOS length
        if offset + 4 > size:
            break
        slen = struct.unpack(">H", data[offset + 2:offset + 4])[0]
        detail = f"scan_data_len={slen}"
        # Find EOI after SOS
        eoi_pos = data.find(b"\xff\xd9", offset + 4)
        next_offset = eoi_pos if eoi_pos != -1 else size
        seg_len = next_offset - offset
    else:
        if offset + 4 > size:
            break
        seg_len = struct.unpack(">H", data[offset + 2:offset + 4])[0]
        next_offset = offset + 2 + seg_len
        detail = f"payload={seg_len - 2} bytes"
    
    segments.append((offset, marker, name, seg_len, detail))
    print(f"0x{offset:06X}   0x{marker:02X}    {name:<30} {seg_len:<8} {detail}")
    
    if marker == 0xD9:  # EOI - stop
        break
    offset = next_offset

print(f"\nTotal segments found: {len(segments)}")
print()

# Check for data after EOI
eoi_pos = data.rfind(b"\xff\xd9")
if eoi_pos != -1 and eoi_pos + 2 < size:
    trailing = size - (eoi_pos + 2)
    print(f"WARNING: {trailing} bytes appended after EOI marker!")
    print(f"Trailing data (hex): {data[eoi_pos+2:eoi_pos+2+min(trailing,64)].hex()}")
else:
    print("OK: No data appended after EOI marker. File ends cleanly.")
print()

# ---------------------------------------------------------------------------
# 2. APP0 / JFIF
# ---------------------------------------------------------------------------
print("-" * 70)
print("APP0 / JFIF DETAILS")
print("-" * 70)
for off, mk, name, slen, det in segments:
    if mk == 0xE0:
        payload = data[off+4:off+2+slen]
        jfif_id = payload[:5]
        version = payload[5:7]
        units = payload[7]
        xd = struct.unpack(">H", payload[8:10])[0]
        yd = struct.unpack(">H", payload[10:12])[0]
        thumb_x = payload[12]
        thumb_y = payload[13]
        print(f"JFIF ID:     {jfif_id}")
        print(f"Version:     {version[0]}.{version[1]}")
        print(f"Units:       {units} (0=no units, 1=ppi, 2=ppcm)")
        print(f"X density:   {xd}")
        print(f"Y density:   {yd}")
        print(f"Thumbnail:   {thumb_x}x{thumb_y} pixels")
        print()

# ---------------------------------------------------------------------------
# 3. APP1 / EXIF DEEP PARSE
# ---------------------------------------------------------------------------
print("-" * 70)
print("APP1 / EXIF DEEP PARSE")
print("-" * 70)

def parse_tiff_ifd(data, base_offset, ifd_offset, tag_names, indent=""):
    """Parse a TIFF IFD at absolute position base_offset + ifd_offset."""
    abs_pos = base_offset + ifd_offset
    if abs_pos + 2 > len(data):
        return
    num_entries = struct.unpack(">H", data[abs_pos:abs_pos+2])[0]
    print(f"{indent}IFD at 0x{abs_pos:06X}, {num_entries} entries")
    entry_end = abs_pos + 2 + num_entries * 12
    if entry_end + 4 > len(data):
        print(f"{indent}  WARNING: IFD extends beyond file bounds")
        return
    
    for i in range(num_entries):
        eoff = abs_pos + 2 + i * 12
        tag, typ, count, val_off = struct.unpack(">HHII", data[eoff:eoff+12])
        tag_name = tag_names.get(tag, f"0x{tag:04X}")
        
        # Type sizes: 1=BYTE,2=ASCII,3=SHORT,4=LONG,5=RATIONAL,7=UNDEFINED,9=SLONG,10=SRATIONAL
        type_sizes = {1:1, 2:1, 3:2, 4:4, 5:8, 7:1, 9:4, 10:8}
        elem_size = type_sizes.get(typ, 1)
        total_bytes = elem_size * count
        
        if total_bytes <= 4:
            value_raw = data[eoff+8:eoff+12]
        else:
            value_abs = base_offset + val_off
            value_raw = data[value_abs:value_abs+min(total_bytes, 64)]
        
        # Decode value
        if typ == 2:  # ASCII
            val_str = value_raw.split(b'\x00')[0].decode('utf-8', errors='replace')
            print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): ASCII = '{val_str}'")
        elif typ == 3:  # SHORT
            if count == 1:
                v = struct.unpack(">H", value_raw[:2])[0]
                print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): SHORT = {v}")
            else:
                vals = struct.unpack(f">{count}H", value_raw[:count*2])
                print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): SHORT[] = {list(vals)}")
        elif typ == 4:  # LONG
            if count == 1:
                v = struct.unpack(">I", value_raw[:4])[0]
                print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): LONG = {v} (0x{v:08X})")
            else:
                vals = struct.unpack(f">{count}I", value_raw[:count*4])
                print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): LONG[] = {list(vals)}")
        elif typ == 5:  # RATIONAL
            if count == 1:
                num, den = struct.unpack(">II", value_raw[:8])
                print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): RATIONAL = {num}/{den} = {num/den:.4f}" if den else f"{num}/0")
            else:
                print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): RATIONAL[] ({count} entries)")
        elif typ == 7:  # UNDEFINED
            print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): UNDEFINED ({total_bytes} bytes) = {value_raw[:32].hex()}")
        elif typ == 1:  # BYTE
            print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): BYTE[] = {list(value_raw[:count])}")
        else:
            print(f"{indent}  Tag 0x{tag:04X} ({tag_name}): TYPE={typ} COUNT={count} VAL={value_raw[:16].hex()}")
    
    # Next IFD pointer
    next_ifd = struct.unpack(">I", data[entry_end:entry_end+4])[0]
    print(f"{indent}Next IFD offset: {next_ifd} (0x{next_ifd:06X})")
    return next_ifd

TIFF_TAGS_IFD0 = {
    0x0100: "ImageWidth", 0x0101: "ImageLength", 0x0102: "BitsPerSample",
    0x0103: "Compression", 0x0106: "PhotometricInterpretation",
    0x0112: "Orientation", 0x0115: "SamplesPerPixel",
    0x011A: "XResolution", 0x011B: "YResolution", 0x0128: "ResolutionUnit",
    0x0131: "Software", 0x0132: "DateTime", 0x013E: "WhitePoint",
    0x013F: "PrimaryChromaticities", 0x0213: "YCbCrCoefficients",
    0x0214: "YCbCrSubSampling", 0x0215: "YCbCrPositioning",
    0x8298: "Copyright", 0x8769: "ExifOffset", 0x8825: "GPSOffset",
}

TIFF_TAGS_EXIF = {
    0x9000: "ExifVersion", 0x9001: "ShutterSpeedValue", 0x9002: "ApertureValue",
    0x9003: "BrightnessValue", 0x9004: "ExposureBiasValue", 0x9005: "MaxApertureValue",
    0x9006: "SubjectDistance", 0x9007: "MeteringMode", 0x9008: "LightSource",
    0x9009: "Flash", 0x900A: "FocalLength", 0x900B: "FlashEnergy",
    0x900C: "SpatialFrequencyResponse", 0x900D: "Noise",
    0x900E: "FocalPlaneXResolution", 0x900F: "FocalPlaneYResolution",
    0x9010: "FocalPlaneResolutionUnit", 0x9011: "ImageNumber",
    0x9012: "SecurityClassification", 0x9013: "ImageHistory",
    0x9014: "SubjectLocation", 0x9015: "ExposureIndex", 0x9016: "TIFF/EPStandardID",
    0x9017: "SensingMethod", 0x9201: "ExposureTime", 0x9202: "FNumber",
    0x9203: "ExposureProgram", 0x9204: "SpectralSensitivity", 0x9205: "GPSInfo",
    0x9206: "ISOSpeedRatings", 0x9207: "OECF", 0x9208: "ExifVersion",
    0x9209: "FlashPixVersion", 0x920A: "ColorSpace", 0x920B: "PixelXDimension",
    0x920C: "PixelYDimension", 0x920D: "RelatedSoundFile",
    0x9214: "SubjectArea", 0x9215: "MakerNote", 0x9216: "UserComment",
    0x9217: "SecTime", 0x9218: "SecTimeDigitized", 0x9219: "SecTime",
    0x9290: "SubSecTime", 0x9291: "SubSecTimeOriginal",
    0x9292: "SubSecTimeDigitized", 0xA000: "FlashpixVersion",
    0xA001: "ColorSpace", 0xA002: "PixelXDimension", 0xA003: "PixelYDimension",
    0xA004: "RelatedSoundFile", 0xA005: "InteroperabilityOffset",
    0xA401: "CustomRendered", 0xA402: "ExposureMode", 0xA403: "WhiteBalance",
    0xA404: "DigitalZoomRatio", 0xA405: "FocalLengthIn35mmFilm",
    0xA406: "SceneCaptureType", 0xA407: "GainControl", 0xA408: "Contrast",
    0xA409: "Saturation", 0xA40A: "Sharpness", 0xA40B: "DeviceSettingDescription",
    0xA40C: "SubjectDistanceRange", 0xA420: "ImageUniqueID",
    0xA430: "CameraOwnerName", 0xA431: "BodySerialNumber",
    0xA432: "LensSpecification", 0xA433: "LensMake", 0xA434: "LensModel",
    0xA435: "LensSerialNumber",
    0x9C9B: "XPTitle", 0x9C9C: "XPComment", 0x9C9D: "XPAuthor",
    0x9C9E: "XPKeywords", 0x9C9F: "XPSubject",
    0xA480: "FlashEnergy", 0xA481: "SpatialFrequencyResponse",
    0xA482: "Noise", 0xA483: "FocalPlaneXResolution",
    0xA484: "FocalPlaneYResolution", 0xA485: "FocalPlaneResolutionUnit",
    0xA486: "ImageNumber", 0xA487: "SecurityClassification",
    0xA488: "ImageHistory", 0xA489: "SubjectLocation",
    0xA48A: "ExposureIndex", 0xA48B: "TIFF/EPStandardID",
    0xA48C: "SensingMethod", 0xA500: "MakerNoteSafety",
}

GPS_TAGS = {
    0x0000: "GPSVersionID", 0x0001: "GPSLatitudeRef", 0x0002: "GPSLatitude",
    0x0003: "GPSLongitudeRef", 0x0004: "GPSLongitude", 0x0005: "GPSAltitudeRef",
    0x0006: "GPSAltitude", 0x0007: "GPSTimeStamp", 0x0008: "GPSSatellites",
    0x0009: "GPSStatus", 0x000A: "GPSMeasureMode", 0x000B: "GPSDOP",
    0x000C: "GPSSpeedRef", 0x000D: "GPSSpeed", 0x000E: "GPSTrackRef",
    0x000F: "GPSTrack", 0x0010: "GPSImgDirectionRef", 0x0011: "GPSImgDirection",
    0x0012: "GPSMapDatum", 0x0013: "GPSDestLatitudeRef", 0x0014: "GPSDestLatitude",
    0x0015: "GPSDestLongitudeRef", 0x0016: "GPSDestLongitude",
    0x0017: "GPSDestBearingRef", 0x0018: "GPSDestBearing",
    0x0019: "GPSDestDistanceRef", 0x001A: "GPSDestDistance",
    0x001B: "GPSProcessingMethod", 0x001C: "GPSAreaInformation",
    0x001D: "GPSDateStamp", 0x001E: "GPSDifferential",
}

for off, mk, name, slen, det in segments:
    if mk == 0xE1:
        payload = data[off+4:off+2+slen]
        print(f"APP1 segment at 0x{off:06X}, length {slen}")
        
        # Check for Exif header
        if payload[:4] == b"Exif\x00\x00":
            tiff_start = off + 4 + 6  # skip "Exif\0\0"
            endian = data[tiff_start:tiff_start+2]
            print(f"  TIFF Endian: {endian} ({'big-endian (MM)' if endian==b'MM' else 'little-endian (II)'})")
            
            # Magic number
            magic = struct.unpack(">H" if endian==b"MM" else "<H", data[tiff_start+2:tiff_start+4])[0]
            print(f"  TIFF Magic:  0x{magic:04X} ({'valid' if magic==42 else 'INVALID!'})")
            
            ifd0_off = struct.unpack(">I" if endian==b"MM" else "<I", data[tiff_start+4:tiff_start+8])[0]
            print(f"  IFD0 offset: {ifd0_off} (absolute 0x{tiff_start+ifd0_off:06X})")
            
            # Parse IFD0
            next_ifd = parse_tiff_ifd(data, tiff_start, ifd0_off, TIFF_TAGS_IFD0, indent="  ")
            
            # Find ExifOffset tag (0x8769) in IFD0
            exif_sub_ifd = None
            gps_ifd = None
            abs_ifd0 = tiff_start + ifd0_off
            num_entries = struct.unpack(">H", data[abs_ifd0:abs_ifd0+2])[0]
            for i in range(num_entries):
                eoff = abs_ifd0 + 2 + i*12
                tag = struct.unpack(">H", data[eoff:eoff+2])[0]
                if tag == 0x8769:
                    typ = struct.unpack(">H", data[eoff+2:eoff+4])[0]
                    cnt = struct.unpack(">I", data[eoff+4:eoff+8])[0]
                    val = struct.unpack(">I", data[eoff+8:eoff+12])[0]
                    exif_sub_ifd = val
                    print(f"\n  ExifSubIFD offset: {val} (absolute 0x{tiff_start+val:06X})")
                elif tag == 0x8825:
                    typ = struct.unpack(">H", data[eoff+2:eoff+4])[0]
                    cnt = struct.unpack(">I", data[eoff+4:eoff+8])[0]
                    val = struct.unpack(">I", data[eoff+8:eoff+12])[0]
                    gps_ifd = val
                    print(f"\n  GPS IFD offset: {val} (absolute 0x{tiff_start+val:06X})")
            
            # Parse ExifSubIFD
            if exif_sub_ifd is not None:
                print("\n  EXIF SUB-IFD:")
                parse_tiff_ifd(data, tiff_start, exif_sub_ifd, TIFF_TAGS_EXIF, indent="    ")
            else:
                print("\n  NO ExifSubIFD found (ExifOffset tag missing or points to NULL)")
            
            # Parse GPS IFD
            if gps_ifd is not None:
                print("\n  GPS IFD:")
                parse_tiff_ifd(data, tiff_start, gps_ifd, GPS_TAGS, indent="    ")
            else:
                print("\n  NO GPS IFD found")
            
            # Check for thumbnail (tags 0x5001, 0x5002 in ExifSubIFD)
            if exif_sub_ifd is not None:
                abs_sub = tiff_start + exif_sub_ifd
                num_sub = struct.unpack(">H", data[abs_sub:abs_sub+2])[0]
                has_thumb = False
                for i in range(num_sub):
                    eoff = abs_sub + 2 + i*12
                    tag = struct.unpack(">H", data[eoff:eoff+2])[0]
                    if tag in (0x5001, 0x5002):
                        has_thumb = True
                        typ = struct.unpack(">H", data[eoff+2:eoff+4])[0]
                        cnt = struct.unpack(">I", data[eoff+4:eoff+8])[0]
                        val = struct.unpack(">I", data[eoff+8:eoff+12])[0]
                        print(f"\n  THUMBNAIL TAG 0x{tag:04X}: offset={val}, count={cnt}, type={typ}")
                        thumb_data = data[tiff_start+val:tiff_start+val+min(cnt,64)]
                        print(f"  Thumbnail data (first 64 bytes): {thumb_data.hex()}")
                if not has_thumb:
                    print("\n  NO thumbnail tags (0x5001/0x5002) found in ExifSubIFD")
        
        # Check for XMP
        elif payload[:29] == b"http://ns.adobe.com/xap/1.0/\0":
            print("  XMP packet found!")
            xmp = payload[29:]
            print(f"  XMP data: {xmp[:200]}")
        else:
            print(f"  Unknown APP1 header: {payload[:16].hex()}")
        print()

# ---------------------------------------------------------------------------
# 4. APP14 / PHOTOSHOP 8BIM DEEP PARSE
# ---------------------------------------------------------------------------
print("-" * 70)
print("APP14 / PHOTOSHOP 8BIM DEEP PARSE")
print("-" * 70)

# 8BIM resource type names (partial)
RESOURCE_IDS = {
    0x03ED: "ImageReady thumbnail",
    0x03EE: "ImageReady flags",
    0x0404: "Print flags / print info",
    0x0407: "URL",
    0x040B: "Print flags information",
    0x040E: "Grid and guides",
    0x0411: "Thumbnail resource",
    0x0417: "Global angle",
    0x041A: "Color samplers",
    0x0421: "ICC Profile",
    0x0422: "Watermark",
    0x0424: "ICC Profile (embedded)",
    0x0425: "Unknown / hash placeholder",
    0x0426: "QuickTime settings",
    0x0427: "Alpha channel names",
    0x0428: "Series of slices",
    0x0429: "Workspace path",
    0x042A: "Print flags",
    0x042B: "Picture package",
    0x042C: "Layer comps",
    0x042D: "Sheet object",
    0x042E: "URL list",
    0x0431: "Grid and guides (layout)",
    0x0432: "Smart object placeholder",
}

for off, mk, name, slen, det in segments:
    if mk == 0xED:
        payload = data[off+4:off+2+slen]
        print(f"APP14 segment at 0x{off:06X}, length {slen}")
        
        # Find "Photoshop 3.0" signature
        sig_idx = payload.find(b"Photoshop 3.0")
        if sig_idx != -1:
            print(f"  'Photoshop 3.0' found at offset {sig_idx} within APP14 payload")
            bim_start = sig_idx + len("Photoshop 3.0") + 1  # +1 for null terminator
            print(f"  8BIM resources start at payload offset {bim_start}")
            
            pos = bim_start
            res_num = 0
            while pos + 8 <= len(payload):
                # Resource signature "8BIM"
                sig = payload[pos:pos+4]
                if sig != b"8BIM":
                    print(f"  No more 8BIM resources at payload offset {pos} (sig={sig})")
                    break
                pos += 4
                res_id = struct.unpack(">H", payload[pos:pos+2])[0]
                pos += 2
                name_len = payload[pos]
                pos += 1
                # Pad name to even bytes
                if name_len > 0:
                    pos += name_len
                if pos % 2 != 0:
                    pos += 1
                size_val = struct.unpack(">I", payload[pos:pos+4])[0]
                pos += 4
                res_data = payload[pos:pos+size_val]
                pos += size_val
                # Pad resource data to even
                if size_val % 2 != 0:
                    pos += 1
                
                res_num += 1
                rname = RESOURCE_IDS.get(res_id, f"Unknown (0x{res_id:04X})")
                print(f"\n  8BIM Resource #{res_num}:")
                print(f"    ID:        0x{res_id:04X} ({res_id}) - {rname}")
                print(f"    Name len:  {name_len}")
                print(f"    Data size: {size_val} bytes")
                if size_val <= 64 and size_val > 0:
                    print(f"    Data:      {res_data.hex()}")
                    try:
                        print(f"    ASCII:     {res_data.decode('utf-8', errors='replace')}")
                    except:
                        pass
                elif size_val > 0:
                    print(f"    Data (first 32): {res_data[:32].hex()}")
                else:
                    print(f"    Data: (empty)")
        else:
            print(f"  No 'Photoshop 3.0' signature found. First bytes: {payload[:32].hex()}")
        print()

# ---------------------------------------------------------------------------
# 5. CHECK FOR OTHER MARKERS / SEGMENTS
# ---------------------------------------------------------------------------
print("-" * 70)
print("OTHER SEGMENTS CHECK (COM, IPTC, XMP, ICC)")
print("-" * 70)
has_com = False
has_iptc = False
has_xmp = False
has_icc = False

for off, mk, name, slen, det in segments:
    if mk == 0xFE:
        has_com = True
        payload = data[off+4:off+2+slen]
        print(f"COM segment at 0x{off:06X}: {payload[:200]}")
    if mk == 0xEE:
        has_iptc = True
        print(f"APP13 (IPTC) at 0x{off:06X}")
    if mk == 0xE2:
        payload = data[off+4:off+2+slen]
        if payload[:12] == b"ICC_PROFILE\0":
            has_icc = True
            print(f"APP2 ICC Profile at 0x{off:06X}, size {slen-2}")
        else:
            print(f"APP2 at 0x{off:06X}: {payload[:16].hex()}")

if not has_com: print("No COM (comment) segments found.")
if not has_iptc: print("No APP13 / IPTC segments found.")
if not has_xmp: print("No XMP packets found in any APP segment.")
if not has_icc: print("No ICC profile found in APP2.")
print()

# ---------------------------------------------------------------------------
# 6. ENTROPY ANALYSIS BY REGION
# ---------------------------------------------------------------------------
print("-" * 70)
print("BYTE ENTROPY ANALYSIS BY REGION")
print("-" * 70)
def shannon_entropy(block):
    if not block:
        return 0
    freq = {}
    for b in block:
        freq[b] = freq.get(b, 0) + 1
    ent = 0
    for count in freq.values():
        p = count / len(block)
        ent -= p * __import__('math').log2(p)
    return ent

# Split image data region (after SOS to before EOI) into chunks
sos_off = None
eoi_off = None
for off, mk, name, slen, det in segments:
    if mk == 0xDA:
        sos_off = off
    if mk == 0xD9:
        eoi_off = off

if sos_off and eoi_off:
    image_data = data[sos_off:eoi_off]
    chunk_size = len(image_data) // 16
    print(f"Image data region: 0x{sos_off:06X} to 0x{eoi_off:06X} ({len(image_data)} bytes)")
    print(f"{'Region':<10} {'Offset':<10} {'Size':<8} {'Entropy':<10} {'Assessment'}")
    print("-" * 70)
    for i in range(16):
        start = i * chunk_size
        end = start + chunk_size if i < 15 else len(image_data)
        chunk = image_data[start:end]
        ent = shannon_entropy(chunk)
        assessment = "Normal" if 6.5 < ent < 8.0 else ("LOW - possible structure" if ent < 6.5 else "HIGH - possible encryption")
        print(f"{i+1:<10} 0x{sos_off+start:06X}  {end-start:<8} {ent:<10.4f} {assessment}")
print()

# ---------------------------------------------------------------------------
# 7. STRING / ARTIFACT SCAN IN METADATA REGIONS ONLY
# ---------------------------------------------------------------------------
print("-" * 70)
print("METADATA-REGION STRING SCAN (not image data)")
print("-" * 70)
meta_regions = []
for off, mk, name, slen, det in segments:
    if mk in (0xE0, 0xE1, 0xE2, 0xED, 0xEE, 0xFE):
        meta_regions.append((off+2, off+2+slen))  # skip marker, include length

for start, end in meta_regions:
    block = data[start:end]
    # Extract ASCII strings length >= 3
    import re
    strings = re.findall(b'[\x20-\x7E]{3,}', block)
    for s in strings:
        print(f"  0x{start:06X}: {s.decode('utf-8', errors='replace')}")

print()
print("=" * 70)
print("DEEP FORENSIC ANALYSIS COMPLETE")
print("=" * 70)
