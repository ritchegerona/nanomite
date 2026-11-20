"""Auto-trigger detection for OSINT / doxxing / identity-mapping tasks.

Scans user input for keywords that indicate an authorized OSINT, identity
mapping, or doxxing-research task, and returns the alias/target terms to
feed into the OSINT orchestrator pipeline.

Authorized security research only.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Optional


# Keywords that indicate an OSINT / identity / doxxing task.
OSINT_KEYWORDS = [
    r"\breal identity\b",
    r"\btrue name\b",
    r"\breal name\b",
    r"\bdoxx\b",
    r"\bdoxing\b",
    r"\bdox\b",
    r"\balias.*real\b",
    r"\bwho is\b.*\b(?:behind|alias|handle)\b",
    r"\bosint\b",
    r"\bopen.source.intelligence\b",
    r"\bsocial.media.profiling\b",
    r"\bemail.harvest\b",
    r"\busername.enumeration\b",
    r"\bidentity.mapping\b",
    r"\balias.*mapping\b",
    r"\bpersona.*research\b",
    r"\bhacker alias\b",
    r"\bhandle.*identity\b",
    r"\bonline persona\b",
    r"\bcyber.*identity\b",
    r"\binvestigate\b",
    r"\binvestigation\b",
    r"\binvestigat\w*\b",
    r"\blook up\b",
    r"\blookup\b",
    r"\bresearch\b",
    r"\bcheck this person\b",
    r"\bcheck\w*\s+this\w*\s+(?:person|handle|alias)\b",
    r"\bfind\w*\s+(?:the\s+)?(?:real|true)\b",
    r"\bunmask\b",
    r"\breveal\w*\s+(?:the\s+)?(?:identity|name|person)\b",
]

OSINT_COMPILED = [re.compile(p, re.IGNORECASE) for p in OSINT_KEYWORDS]

# Common English words to filter out during alias extraction
_STOPWORDS = {
    "the", "and", "for", "with", "from", "this", "that", "has", "was", "are",
    "find", "the", "real", "name", "identity", "who", "what", "where", "when",
    "why", "how", "can", "could", "would", "should", "please", "help", "me",
    "my", "your", "his", "her", "its", "our", "their", "this", "these", "those",
    "investigate", "investigation", "research", "check", "person", "handle",
    "alias", "look", "lookup", "unmask", "reveal", "this", "of", "a", "an",
    "doxx", "doxing", "dox", "behind", "the", "person",
}


@dataclass
class OsintTriggerResult:
    """Result of scanning text for OSINT trigger keywords."""

    triggered: bool
    matched_patterns: list[str] = field(default_factory=list)
    extracted_aliases: list[str] = field(default_factory=list)
    confidence: float = 0.0


class OsintTrigger:
    """Detects OSINT tasks in natural-language input."""

    def __init__(self, keywords: Optional[list[str]] = None):
        self.patterns = [re.compile(p, re.IGNORECASE) for p in (keywords or OSINT_KEYWORDS)]

    def scan(self, text: str) -> OsintTriggerResult:
        """Scan ``text`` for OSINT trigger keywords."""
        matched: list[str] = []
        for pat in self.patterns:
            if pat.search(text):
                matched.append(pat.pattern)

        triggered = len(matched) > 0
        aliases = self._extract_aliases(text) if triggered else []
        confidence = min(1.0, 0.4 + 0.2 * len(matched))

        return OsintTriggerResult(
            triggered=triggered,
            matched_patterns=matched,
            extracted_aliases=aliases,
            confidence=confidence,
        )

    @staticmethod
    def _extract_aliases(text: str) -> list[str]:
        """Heuristically extract potential alias/handle tokens from text.

        Priority:
        1. Quoted phrases ("DNH Klammer")
        2. Capitalized 2-word phrases (DNH Klammer, Rodel Lablab)
        3. Single handles with numbers/underscores/dots (e.g., "klammer", "0xKlammerV2", "DNH.klammer")
        """
        aliases: list[str] = []
        consumed_spans: list[tuple[int, int]] = []

        def _add(token: str) -> None:
            if token and token.lower() not in _STOPWORDS and token not in aliases:
                aliases.append(token)

        # 1. Quoted handles like "DNH Klammer"
        for m in re.finditer(r'["\']([^\'\"]{2,60})["\']', text):
            _add(m.group(1).strip())
            consumed_spans.append((m.start(), m.end()))

        # 2. Capitalized multiword aliases — prefer exactly 2 words
        for m in re.finditer(
            r"\b([A-Z][a-zA-Z]+)\s+([A-Z][a-zA-Z]+)\b", text
        ):
            token = f"{m.group(1)} {m.group(2)}"
            # Skip if this span is inside a quoted match already consumed
            if any(m.start() >= s and m.end() <= e for s, e in consumed_spans):
                continue
            _add(token)
            consumed_spans.append((m.start(), m.end()))

        # 3. Single handles with numbers/underscores/dots (e.g., "klammer", "0xKlammerV2", "DNH.klammer")
        for m in re.finditer(
            r"\b(?:0x)?[A-Za-z][A-Za-z0-9_.]{2,30}\b", text
        ):
            # Skip if this token is inside a multiword match already consumed
            if any(m.start() >= s and m.end() <= e for s, e in consumed_spans):
                continue
            token = m.group(0)
            _add(token)

        return aliases[:10]


def should_run_osint(text: str) -> OsintTriggerResult:
    """Convenience: scan ``text`` and return trigger result."""
    return OsintTrigger().scan(text)


__all__ = ["OsintTrigger", "OsintTriggerResult", "should_run_osint", "OSINT_KEYWORDS"]
