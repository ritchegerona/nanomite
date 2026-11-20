"""Metadata extraction from images, documents, and binary files.

Extracts EXIF (GPS, camera, timestamps), document metadata (author,
creation date, software), and embedded strings from files for
authorized forensic research.
"""

from __future__ import annotations

import subprocess
import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class MetadataResult:
    file_path: str
    format: Optional[str] = None
    exif: dict = field(default_factory=dict)
    document: dict = field(default_factory=dict)
    strings: list[str] = field(default_factory=list)
    gps: Optional[dict] = None  # {lat, lon, alt}
    timestamps: dict = field(default_factory=dict)  # creation, modify, etc.
    software: Optional[str] = None
    author: Optional[str] = None

    def to_dict(self) -> dict:
        return {
            "file_path": self.file_path,
            "format": self.format,
            "exif": self.exif,
            "document": self.document,
            "strings": self.strings,
            "gps": self.gps,
            "timestamps": self.timestamps,
            "software": self.software,
            "author": self.author,
        }


class MetadataExtractor:
    """Extract metadata using exiftool when available, with fallbacks."""

    def __init__(self):
        self._exiftool_available = self._check_exiftool()

    @staticmethod
    def _check_exiftool() -> bool:
        try:
            subprocess.run(["exiftool", "-ver"], capture_output=True, check=True, timeout=5)
            return True
        except (FileNotFoundError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
            return False

    def extract(self, file_path: str | Path) -> MetadataResult:
        path = Path(file_path)
        if not path.exists():
            raise FileNotFoundError(f"File not found: {path}")

        result = MetadataResult(file_path=str(path))

        if self._exiftool_available:
            self._extract_with_exiftool(path, result)
        else:
            self._extract_fallback(path, result)

        return result

    def _extract_with_exiftool(self, path: Path, result: MetadataResult) -> None:
        try:
            out = subprocess.run(
                ["exiftool", "-json", str(path)],
                capture_output=True, text=True, timeout=30,
            )
            if out.returncode == 0 and out.stdout.strip():
                data = json.loads(out.stdout)[0]
                result.format = data.get("FileType")
                # GPS
                if "GPSPosition" in data:
                    result.gps = {"raw": data["GPSPosition"]}
                if "GPSLatitude" in data and "GPSLongitude" in data:
                    result.gps = {
                        "lat": data["GPSLatitude"],
                        "lon": data["GPSLongitude"],
                        "alt": data.get("GPSAltitude"),
                    }
                # Timestamps
                for key in ["CreateDate", "ModifyDate", "DateTimeOriginal", "MetadataDate"]:
                    if key in data:
                        result.timestamps[key.lower()] = data[key]
                # Software / Author
                result.software = data.get("Software") or data.get("Producer")
                result.author = data.get("Author") or data.get("Artist")
                result.exif = {k: v for k, v in data.items() if k not in ("SourceFile",)}
        except Exception:
            pass

    def _extract_fallback(self, path: Path, result: MetadataResult) -> None:
        """Fallback: read file header/strings for basic metadata."""
        try:
            data = path.read_bytes()[:200_000]  # first 200KB
            # crude string extraction for emails, URLs, names
            import re
            strings = re.findall(rb"[ -~]{4,}", data)
            result.strings = [s.decode("utf-8", errors="ignore") for s in strings[:50]]
        except Exception:
            pass