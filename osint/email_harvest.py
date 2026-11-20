"""Public email harvesting utilities for authorized OSINT research.

Extracts email addresses from publicly accessible web pages and
search-engine snippets. Does not scrape private directories or
perform credentialed access.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Optional


EMAIL_REGEX = re.compile(
    r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}"
)


@dataclass
class EmailRecord:
    email: str
    source: str
    source_url: Optional[str]
    context: Optional[str] = None  # surrounding text
    confidence: float = 0.7

    def to_dict(self) -> dict:
        return {
            "email": self.email,
            "source": self.source,
            "source_url": self.source_url,
            "context": self.context,
            "confidence": self.confidence,
        }


class EmailHarvester:
    """Harvest email addresses from public text content."""

    @staticmethod
    def extract_from_text(text: str, source: str, source_url: Optional[str] = None) -> list[EmailRecord]:
        found: list[EmailRecord] = []
        for match in EMAIL_REGEX.finditer(text):
            email = match.group(0).lower()
            start = max(0, match.start() - 80)
            end = min(len(text), match.end() + 80)
            context = text[start:end].replace("\n", " ").strip()
            found.append(
                EmailRecord(
                    email=email,
                    source=source,
                    source_url=source_url,
                    context=context,
                    confidence=0.7,
                )
            )
        return found

    @staticmethod
    def deduplicate(records: list[EmailRecord]) -> list[EmailRecord]:
        seen: set[str] = set()
        unique: list[EmailRecord] = []
        for rec in records:
            if rec.email not in seen:
                seen.add(rec.email)
                unique.append(rec)
        return unique