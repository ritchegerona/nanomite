"""CTI-grade report generation for cracking/audit operations.

Produces machine-readable after-action artifacts suitable for inclusion in
threat-intelligence reports, SIEM ingestion, or client deliverables.

Follows the advanced standard:
* frozen slotted dataclasses for report models
* pure formatting separated from I/O
* module-level logging
* full type hints
* SARIF / JSON / Markdown output
"""

from __future__ import annotations

import json
import logging
from collections.abc import Iterator
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from .engine import CrackOutcome, CrackResult

logger = logging.getLogger(__name__)


@dataclass(frozen=True, slots=True)
class Engagement:
    """Authorized engagement metadata bound to all results."""

    engagement_id: str
    scope_id: str
    operator: str
    authorized_at: str
    targets: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class CrackEvidence:
    """Immutable evidence record for one cracked hash."""

    target_path: str
    scope_id: str
    hash_type: str
    hash_value: str
    plaintext: str
    attempts: int
    elapsed_ms: int
    timestamp_utc: str
    engagement_id: str


@dataclass(frozen=True, slots=True)
class CtiReport:
    """Aggregate CTI report: engagement + evidence + summary."""

    engagement: Engagement
    evidence: tuple[CrackEvidence, ...]

    @property
    def cracked_count(self) -> int:
        return len(self.evidence)

    @property
    def total_attempts(self) -> int:
        return sum(e.attempts for e in self.evidence)

    @property
    def scope_ids(self) -> Iterator[str]:
        return (e.scope_id for e in self.evidence)


def evidence_from_result(
    *,
    result: CrackResult,
    engagement: Engagement,
    target_path: str,
    hash_type: str,
    hash_value: str,
) -> CrackEvidence:
    """Pure constructor: build evidence from a successful crack result."""
    if result.outcome is not CrackOutcome.SUCCESS or result.plaintext is None:
        msg = "evidence requires a successful crack result"
        raise ValueError(msg)
    return CrackEvidence(
        target_path=target_path,
        scope_id=result.scope_id,
        hash_type=hash_type,
        hash_value=hash_value,
        plaintext=result.plaintext,
        attempts=result.attempts,
        elapsed_ms=result.elapsed_ms,
        timestamp_utc=result.timestamp_utc,
        engagement_id=engagement.engagement_id,
    )


def to_json(report: CtiReport) -> str:
    """Pure serialization to CTI JSON. No I/O."""
    payload = {
        "report_type": "crack_audit",
        "format_version": "1.0",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "engagement": {
            "engagement_id": report.engagement.engagement_id,
            "scope_id": report.engagement.scope_id,
            "operator": report.engagement.operator,
            "authorized_at": report.engagement.authorized_at,
            "targets": list(report.engagement.targets),
        },
        "summary": {
            "cracked_count": report.cracked_count,
            "total_attempts": report.total_attempts,
            "scope_ids": sorted(set(report.scope_ids)),
        },
        "evidence": [
            {
                "target_path": e.target_path,
                "scope_id": e.scope_id,
                "hash_type": e.hash_type,
                "hash_value": e.hash_value,
                "plaintext": e.plaintext,
                "attempts": e.attempts,
                "elapsed_ms": e.elapsed_ms,
                "timestamp_utc": e.timestamp_utc,
                "engagement_id": e.engagement_id,
            }
            for e in report.evidence
        ],
    }
    return json.dumps(payload, indent=2)


def to_sarif(report: CtiReport) -> str:
    """Pure serialization to SARIF 2.1.0 for SIEM ingestion. No I/O."""
    runs = [
        {
            "tool": {
                "driver": {
                    "name": "nanomite-crack",
                    "version": "1.0.0",
                    "rules": [
                        {
                            "id": "CRACKED-HASH",
                            "name": "Weak / cracked password hash",
                            "shortDescription": {
                                "text": "A hash was cracked within the authorized attempt cap."
                            },
                            "properties": {
                                "security-severity": "7.3",
                                "tags": ["credential-access", "cracked-hash"],
                            },
                        }
                    ],
                }
            },
            "results": [
                {
                    "ruleId": "CRACKED-HASH",
                    "level": "warning",
                    "message": {
                        "text": (
                            f"Cracked hash {e.hash_value} -> {e.plaintext} "
                            f"in {e.attempts} attempt(s) on {e.target_path}"
                        )
                    },
                    "locations": [
                        {
                            "physicalLocation": {
                                "artifactLocation": {"uri": e.target_path}
                            }
                        }
                    ],
                    "properties": {
                        "scope_id": e.scope_id,
                        "engagement_id": e.engagement_id,
                        "attempts": e.attempts,
                        "elapsed_ms": e.elapsed_ms,
                        "timestamp_utc": e.timestamp_utc,
                    },
                }
                for e in report.evidence
            ],
        }
    ]
    return json.dumps(
        {
            "$schema": "https://schemastore.github.io/json/sarif-2.1.0-rtm.5",
            "version": "2.1.0",
            "runs": runs,
        },
        indent=2,
    )


def to_markdown(report: CtiReport) -> str:
    """Pure human-readable Markdown rendering. No I/O."""
    lines = [
        f"# Crack Audit Report — {report.engagement.engagement_id}",
        "",
        f"**Engagement ID:** {report.engagement.engagement_id}",
        f"**Scope ID:** {report.engagement.scope_id}",
        f"**Operator:** {report.engagement.operator}",
        f"**Authorized at:** {report.engagement.authorized_at}",
        f"**Targets:** {', '.join(report.engagement.targets) or 'none'}",
        "",
        "## Summary",
        f"- Cracked hashes: {report.cracked_count}",
        f"- Total attempts: {report.total_attempts}",
        f"- Scope IDs: {', '.join(sorted(set(report.scope_ids))) or 'none'}",
        "",
    ]
    if report.evidence:
        lines.append("## Evidence")
        lines.append(
            "| Target | Hash Type | Hash Value | Plaintext | Attempts | Elapsed (ms) | Scope |"
        )
        lines.append(
            "|--------|-----------|------------|-----------|----------|--------------|-------|"
        )
        for e in report.evidence:
            lines.append(
                f"| {e.target_path} | {e.hash_type} | {e.hash_value} | "
                f"{e.plaintext} | {e.attempts} | {e.elapsed_ms} | {e.scope_id} |"
            )
    else:
        lines.append("## Evidence")
        lines.append("_No plaintext recovered within the authorized attempt cap._")
    return "\n".join(lines)


def write_report(report: CtiReport, path: str, fmt: str = "json") -> None:
    """Write a CTI report to disk. I/O boundary only."""
    content = {
        "json": to_json,
        "sarif": to_sarif,
        "md": to_markdown,
        "markdown": to_markdown,
    }[fmt](report)
    p = Path(path)
    p.write_text(content, encoding="utf-8")
    logger.info(
        "cti report written",
        extra={
            "path": str(p),
            "format": fmt,
            "engagement_id": report.engagement.engagement_id,
            "cracked_count": report.cracked_count,
        },
    )
