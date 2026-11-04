"""Structured report formatting for crack results.

Follows the advanced standard:
* frozen slotted dataclasses for report models
* pure formatting functions separated from I/O
* module-level logging
* full type hints
* JSON and plain-text renderers
"""

from __future__ import annotations

import json
import logging
from collections.abc import Iterator
from dataclasses import dataclass

from .engine import CrackOutcome, CrackResult

logger = logging.getLogger(__name__)


@dataclass(frozen=True, slots=True)
class CrackReport:
    """Immutable aggregate of crack results for one engagement."""

    scope_id: str
    results: tuple[CrackResult, ...]

    @property
    def successes(self) -> Iterator[CrackResult]:
        return (r for r in self.results if r.outcome is CrackOutcome.SUCCESS)

    @property
    def failures(self) -> Iterator[CrackResult]:
        return (r for r in self.results if r.outcome is CrackOutcome.FAILURE)

    @property
    def skipped(self) -> Iterator[CrackResult]:
        return (r for r in self.results if r.outcome is CrackOutcome.SKIPPED)

    @property
    def total_attempts(self) -> int:
        return sum(r.attempts for r in self.results)

    @property
    def success_count(self) -> int:
        return sum(1 for _ in self.successes)


def to_json(report: CrackReport) -> str:
    """Pure serialization to JSON. No I/O."""
    payload = {
        "scope_id": report.scope_id,
        "total_results": len(report.results),
        "success_count": report.success_count,
        "total_attempts": report.total_attempts,
        "successes": [
            {
                "target_path": r.target_path,
                "plaintext": r.plaintext,
                "attempts": r.attempts,
                "elapsed_ms": r.elapsed_ms,
                "timestamp_utc": r.timestamp_utc,
            }
            for r in report.successes
        ],
    }
    return json.dumps(payload, indent=2)


def to_text(report: CrackReport) -> str:
    """Pure human-readable rendering. No I/O."""
    lines = [
        f"Crack audit report — scope: {report.scope_id}",
        f"  total results: {len(report.results)}",
        f"  successes: {report.success_count}",
        f"  total attempts: {report.total_attempts}",
    ]
    for r in report.successes:
        lines.append(
            f"  CRACKED {r.target_path}: {r.plaintext} "
            f"(attempt {r.attempts}, {r.elapsed_ms}ms)"
        )
    if report.success_count == 0:
        lines.append("  no plaintext recovered within attempt cap")
    return "\n".join(lines)


def write_report(report: CrackReport, path: str, fmt: str = "text") -> None:
    """Write a report to disk. I/O boundary only."""
    from pathlib import Path

    content = to_json(report) if fmt == "json" else to_text(report)
    p = Path(path)
    p.write_text(content, encoding="utf-8")
    logger.info(
        "report written",
        extra={"path": str(p), "format": fmt, "success_count": report.success_count},
    )
