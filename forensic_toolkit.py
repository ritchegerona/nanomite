#!/usr/bin/env python3
"""
NANOMITE Forensic Toolkit - Unified wrapper for all forensic & metadata tools
Combines system tools, Python libraries, and custom analysis scripts.
"""
import subprocess
import sys
import os
from pathlib import Path

TOOLKIT_DIR = Path(__file__).parent
VENV_PYTHON = TOOLKIT_DIR / "forensic-venv" / "bin" / "python"

# System tools available (installed via apt)
SYSTEM_TOOLS = {
    "exiftool": "Metadata extraction (images, docs, audio, video)",
    "jhead": "JPEG header manipulation",
    "identify": "ImageMagick - image info",
    "gm": "GraphicsMagick - image info",
    "binwalk": "Firmware/embedded file analysis",
    "foremost": "File carving",
    "scalpel": "File carving (config-based)",
    "steghide": "Steganography hide/extract",
    "stegsnow": "Whitespace steganography",
    "outguess": "JPEG steganography",
    "zsteg": "PNG/BMP steganography",
    "jpegdump": "JPEG structure dump",
    "jpeginfo": "JPEG info/error check",
    "qpdf": "PDF analysis/manipulation",
    "pdf-parser": "PDF structure parsing",
    "pdfid": "PDF identifier/suspicious elements",
    "pdfinfo": "PDF metadata",
    "pdftotext": "PDF text extraction",
    "pdftk": "PDF toolkit",
    "xxd": "Hex dump",
    "hexdump": "Hex dump (alt)",
    "fls": "Sleuth Kit - file listing",
    "icat": "Sleuth Kit - inode content",
    "istat": "Sleuth Kit - inode stats",
    "fsstat": "Sleuth Kit - filesystem stats",
}

# Python packages in venv
PYTHON_PACKAGES = {
    "exifread": "EXIF/TIFF metadata reading",
    "PIL/Pillow": "Image processing, ELA, format conversion",
    "numpy": "Numerical analysis, noise statistics",
    "sklearn": "ML for forensics (classification, clustering)",
    "onnxruntime": "ONNX model inference for trained detectors",
    "python-docx": "DOCX metadata, content, hidden text extraction",
    "openpyxl": "XLSX metadata, hidden sheets, formulas, comments",
    "python-pptx": "PPTX metadata, hidden slides, notes",
    "pdfplumber": "PDF text, tables, metadata extraction",
    "pypdf2": "PDF structure, JavaScript, annotations",
}

# Custom scripts
CUSTOM_SCRIPTS = {
    "ela_analysis.py": "Error Level Analysis (ELA) for JPEG manipulation detection",
    "ela_stats.py": "ELA grid statistics for region analysis",
    "noise_check.py": "Noise consistency analysis across image quadrants",
    "osin_deep_forensic.py": "Deep JPEG forensic: segments, EXIF, 8BIM, entropy, strings",
}

def check_tool(tool):
    """Check if a system tool is available."""
    return subprocess.run(["which", tool], capture_output=True).returncode == 0

def run_tool(tool, args, input_file=None):
    """Run a system tool with args."""
    cmd = [tool] + args
    if input_file:
        cmd.append(str(input_file))
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    return result.returncode, result.stdout, result.stderr

def run_python_script(script, args=None):
    """Run a custom Python script from the toolkit."""
    script_path = TOOLKIT_DIR / script
    if not script_path.exists():
        return 1, "", f"Script not found: {script_path}"
    cmd = [str(VENV_PYTHON), str(script_path)]
    if args:
        cmd.extend(args)
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    return result.returncode, result.stdout, result.stderr

def forensic_analyze(filepath, output_dir=None):
    """Run comprehensive forensic analysis on a file."""
    fp = Path(filepath)
    if not fp.exists():
        return {"error": f"File not found: {filepath}"}
    
    output_dir = Path(output_dir) if output_dir else TOOLKIT_DIR / "forensic_output" / fp.stem
    output_dir.mkdir(parents=True, exist_ok=True)

    # Remove temp_* script copies and stale per-script outputs left by
    # earlier runs so a clean run never mixes old results in.
    for leftover in output_dir.glob("temp_*.py"):
        leftover.unlink()
    for script in CUSTOM_SCRIPTS:
        for stale in (output_dir / f"{script}.out", output_dir / f"{script}.err"):
            stale.unlink(missing_ok=True)
    
    results = {
        "file": str(fp),
        "size": fp.stat().st_size,
        "system_tools": {},
        "python_analysis": {},
        "custom_scripts": {},
    }
    
    # System tool analysis
    print(f"[+] Running system tools on {fp.name}...")
    
    # exiftool - comprehensive metadata
    if check_tool("exiftool"):
        code, out, err = run_tool("exiftool", ["-a", "-u", "-g1"], fp)
        results["system_tools"]["exiftool"] = {"code": code, "output": out, "error": err}
        (output_dir / "exiftool.txt").write_text(out)
    
    # jhead for JPEG
    if fp.suffix.lower() in [".jpg", ".jpeg"] and check_tool("jhead"):
        code, out, err = run_tool("jhead", ["-v"], fp)
        results["system_tools"]["jhead"] = {"code": code, "output": out, "error": err}
        (output_dir / "jhead.txt").write_text(out)
    
    # ImageMagick identify
    if check_tool("identify"):
        code, out, err = run_tool("identify", ["-verbose"], fp)
        results["system_tools"]["identify"] = {"code": code, "output": out, "error": err}
        (output_dir / "identify.txt").write_text(out)
    
    # binwalk for embedded files
    if check_tool("binwalk"):
        code, out, err = run_tool("binwalk", ["-e", "--run-as=root"], fp)
        results["system_tools"]["binwalk"] = {"code": code, "output": out, "error": err}
        (output_dir / "binwalk.txt").write_text(out)
    
    # jpegdump for JPEG structure
    if fp.suffix.lower() in [".jpg", ".jpeg"] and check_tool("jpegdump"):
        code, out, err = run_tool("jpegdump", ["-a"], fp)
        results["system_tools"]["jpegdump"] = {"code": code, "output": out, "error": err}
        (output_dir / "jpegdump.txt").write_text(out)
    
    # PDF tools
    if fp.suffix.lower() == ".pdf":
        for tool in ["pdfinfo", "pdfid", "pdf-parser"]:
            if check_tool(tool):
                code, out, err = run_tool(tool, [], fp)
                results["system_tools"][tool] = {"code": code, "output": out, "error": err}
                (output_dir / f"{tool}.txt").write_text(out)
    
    # DOCX analysis
    if fp.suffix.lower() == ".docx":
        code, out, err = run_python_script("docx_analysis.py", [str(fp), str(output_dir)])
        results["system_tools"]["docx_analysis"] = {"code": code, "output": out, "error": err}
    
    # XLSX analysis
    if fp.suffix.lower() == ".xlsx":
        code, out, err = run_python_script("xlsx_analysis.py", [str(fp), str(output_dir)])
        results["system_tools"]["xlsx_analysis"] = {"code": code, "output": out, "error": err}
    
    # PPTX analysis
    if fp.suffix.lower() == ".pptx":
        code, out, err = run_python_script("pptx_analysis.py", [str(fp), str(output_dir)])
        results["system_tools"]["pptx_analysis"] = {"code": code, "output": out, "error": err}
    
    # Custom image-forensics scripts. They are arg-driven now
    # (`script <target> [output_dir]`) and only make sense on raster images,
    # so non-image targets (docx/pptx/pdf/...) skip them instead of crashing
    # with a PIL traceback.
    print("[+] Running custom Python analysis...")
    image_exts = {".jpg", ".jpeg", ".png", ".bmp", ".gif", ".tif", ".tiff", ".webp"}
    if fp.suffix.lower() in image_exts:
        for script in CUSTOM_SCRIPTS:
            if not (TOOLKIT_DIR / script).exists():
                results["custom_scripts"][script] = {"error": f"script not found: {script}"}
                continue
            code, out, err = run_python_script(script, [str(fp), str(output_dir)])
            results["custom_scripts"][script] = {"code": code, "output": out, "error": err}
            (output_dir / f"{script}.out").write_text(out)
            if err:
                (output_dir / f"{script}.err").write_text(err)
    else:
        results["custom_scripts"]["skipped"] = (
            "image-only scripts skipped: target is not a raster image"
        )
    
    # Save summary
    import json
    (output_dir / "analysis_summary.json").write_text(json.dumps(results, indent=2))
    
    print(f"[+] Analysis complete. Results in: {output_dir}")
    return results

def list_available_tools():
    """Print all available forensic tools."""
    print("=" * 70)
    print("NANOMITE FORENSIC TOOLKIT - AVAILABLE TOOLS")
    print("=" * 70)
    
    print("\n[SYSTEM TOOLS]")
    for tool, desc in SYSTEM_TOOLS.items():
        status = "✓" if check_tool(tool) else "✗"
        print(f"  {status} {tool:<20} - {desc}")
    
    print("\n[PYTHON PACKAGES] (in forensic-venv)")
    for pkg, desc in PYTHON_PACKAGES.items():
        try:
            __import__(pkg.split("/")[0])
            status = "✓"
        except ImportError:
            status = "✗"
        print(f"  {status} {pkg:<20} - {desc}")
    
    print("\n[CUSTOM SCRIPTS]")
    for script, desc in CUSTOM_SCRIPTS.items():
        path = TOOLKIT_DIR / script
        status = "✓" if path.exists() else "✗"
        print(f"  {status} {script:<30} - {desc}")
    
    print("\n[VENV PYTHON]")
    print(f"  {VENV_PYTHON}")

if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Nanomite Forensic Toolkit")
    parser.add_argument("file", nargs="?", help="File to analyze")
    parser.add_argument("-l", "--list", action="store_true", help="List available tools")
    parser.add_argument("-o", "--output", help="Output directory")
    args = parser.parse_args()
    
    if args.list or not args.file:
        list_available_tools()
        sys.exit(0)
    
    forensic_analyze(args.file, args.output)