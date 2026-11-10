# NANOMITE Forensic Toolkit

## Overview
Complete forensic and metadata analysis toolkit integrated with nanomite.

## Components Installed

### Python Virtual Environment (`forensic-venv/`)
- **exifread** - EXIF/TIFF metadata extraction
- **Pillow** - Image processing, ELA, format conversion
- **numpy** - Numerical analysis, noise statistics
- **scikit-learn** - ML for forensics (classification, clustering)
- **onnxruntime** - ONNX model inference for trained detectors
- **pytest, mypy, ruff** - Development tools

### Custom Analysis Scripts
| Script | Purpose |
|--------|---------|
| `ela_analysis.py` | Error Level Analysis for JPEG manipulation detection |
| `ela_stats.py` | ELA grid statistics (8x8 regions) |
| `noise_check.py` | Noise consistency across image quadrants |
| `osin_deep_forensic.py` | Deep JPEG forensic: segments, EXIF, 8BIM, entropy, strings |

### Unified Wrapper
`forensic_toolkit.py` - Runs all available tools on a target file

## Usage

```bash
# List all available tools
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/forensic_toolkit.py --list

# Analyze a file (comprehensive)
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/forensic_toolkit.py /path/to/image.jpg

# Analyze with custom output directory
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/forensic_toolkit.py /path/to/file -o /path/to/output
```

## Output
Results saved to `forensic_output/<filename>/` containing:
- `analysis_summary.json` - Complete structured results
- `exiftool.txt` - Full metadata (if exiftool installed)
- `identify.txt` - ImageMagick analysis
- `binwalk.txt` - Embedded file analysis
- `jpegdump.txt` - JPEG structure
- Custom script outputs (`.out` files)
- Generated images (ELA, resaved)

## Install System Tools (requires sudo)
```bash
sudo /home/ritche/nanomite/install_forensic_tools.sh
```

This installs:
- exiftool, jhead, imagemagick, graphicsmagick
- binwalk, foremost, scalpel (file carving)
- steghide, stegsnow, outguess, zsteg (steganography)
- jpegdump, jpeginfo, qpdf, pdf-parser, pdfid (PDF/image analysis)
- sleuthkit, autopsy, testdisk (filesystem forensics)
- hashdeep, ssdeep (hashing/fuzzy matching)
- tesseract, ffmpeg, mediainfo, exiv2, mat2
- Python packages: exifread, pillow, numpy, scikit-learn, onnxruntime, etc.

## Quick Commands

```bash
# Run deep forensic on an image
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/osin_deep_forensic.py /path/to/image.jpg

# Run ELA analysis (writes resaved.jpg + ela.png into the output dir, or next to the image)
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/ela_analysis.py /path/to/image.jpg [output_dir]

# Run ELA grid statistics (reads ela.png from the output dir)
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/ela_stats.py /path/to/image.jpg [output_dir]

# Run noise check
/home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/noise_check.py /path/to/image.jpg
```

All custom scripts take the target as an argument; none contain hardcoded
paths anymore. Non-image targets skip the image scripts automatically.

## Integration with nanomite
The toolkit can be invoked from nanomite agent:
```
/run python /home/ritche/nanomite/forensic_toolkit.py /path/to/file
```
