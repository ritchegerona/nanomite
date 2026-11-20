"""Report generation module.

Produces Markdown and JSON reports from InvestigationReport objects.
Pure computation with file I/O limited to writing output.
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

from ..config.settings import Settings
from ..core.models import InvestigationReport, TargetProfile

logger = logging.getLogger(__name__)


def _format_confidence(score: float) -> str:
    """Format a confidence score as a percentage string."""
    return f"{score * 100:.1f}%"


def _profile_to_markdown(alias: str, profile: TargetProfile) -> str:
    """Generate a Markdown section for a single target profile."""
    lines: list[str] = []

    lines.append(f"## Target: {alias}")
    lines.append("")

    # Identification
    lines.append("### Identification")
    lines.append(f"- **Real Name Candidate(s):** {', '.join(profile.real_name_candidates) or 'Not found'}")
    lines.append(f"- **Real Name Confidence:** {_format_confidence(profile.real_name_confidence)}")
    lines.append(f"- **Aggregate Confidence:** {_format_confidence(profile.aggregate_confidence)}")
    lines.append(f"- **Classification:** {profile.classification.upper()}")
    lines.append(f"- **Last Updated:** {profile.last_updated}")
    lines.append("")

    # Location
    lines.append("### Location Profile")
    loc = profile.location
    lines.append(f"- **Country:** {loc.country or 'Unknown'}")
    lines.append(f"- **Region:** {loc.region or 'Unknown'}")
    lines.append(f"- **City:** {loc.city or 'Unknown'}")
    lines.append(f"- **Basis:** {loc.basis or 'No location data'}")
    lines.append(f"- **Confidence:** {_format_confidence(loc.confidence)}")
    lines.append("")

    # Digital Presence
    lines.append("### Digital Presence")
    if not profile.platforms:
        lines.append("- No platform presence detected.")
    else:
        lines.append("| Platform | Handle | User ID | URL | Accessible | Notes |")
        lines.append("|----------|--------|---------|-----|------------|-------|")
        for p in profile.platforms:
            lines.append(
                f"| {p.platform} | {p.handle or 'N/A'} | {p.user_id or 'N/A'} | "
                f"{p.url or 'N/A'} | {'Yes' if p.accessible else 'No'} | {p.notes or '-'} |"
            )
    lines.append("")

    # Evidence Chain
    lines.append("### Evidence Chain")
    if not profile.evidence:
        lines.append("- No evidence collected.")
    else:
        lines.append("| # | Source | Data Type | Value | Weight | Confidence | Weighted Score |")
        lines.append("|---|--------|-----------|-------|--------|------------|----------------|")
        for i, e in enumerate(profile.evidence, 1):
            lines.append(
                f"| {i} | {e.source} | {e.data_type} | {e.value[:60]}{'...' if len(e.value) > 60 else ''} | "
                f"{e.weight} | {e.confidence} | {e.weighted_score()} |"
            )
    lines.append("")

    # Infrastructure
    if profile.infrastructure:
        lines.append("### Infrastructure")
        lines.append("| Domain | Registrar/IP |")
        lines.append("|--------|-------------|")
        for domain, registrar in profile.infrastructure.items():
            lines.append(f"| {domain} | {registrar} |")
        lines.append("")

    # Recommendations
    lines.append("### Recommended Next Steps")
    if profile.classification == "confirmed":
        lines.append("- Identity confirmed. Proceed to authorized action per scope.")
    elif profile.classification == "high_candidate":
        lines.append("- Verify with additional independent source before action.")
    elif profile.classification == "moderate_candidate":
        lines.append("- Continue passive reconnaissance; attempt authenticated access.")
    elif profile.classification == "weak_lead":
        lines.append("- Low priority. Revisit if new seeds emerge.")
    else:
        lines.append("- Unresolved. Requires authenticated platform access or law enforcement records.")
        lines.append("- Consider: Facebook Page Transparency, Instagram login, Telegram infiltration, NBI/NPC records.")

    lines.append("")
    lines.append("---")
    lines.append("")

    return "\n".join(lines)


def generate_markdown_report(report: InvestigationReport) -> str:
    """Generate a complete Markdown report."""
    lines: list[str] = []

    lines.append("# OSINT Profiling Report")
    lines.append("")
    lines.append(f"**Methodology:** {report.methodology}")
    lines.append(f"**Generated At:** {report.generated_at}")
    lines.append("")

    # Summary
    lines.append("## Executive Summary")
    lines.append(f"- **Total Targets:** {report.total_targets}")
    lines.append(f"- **Confirmed:** {report.confirmed_count}")
    lines.append(f"- **Unresolved:** {report.unresolved_count}")
    lines.append("")

    # Confidence Matrix
    lines.append("## Confidence Matrix")
    lines.append("| Target | Real Name Candidate | Confidence | Classification | Location |")
    lines.append("|--------|---------------------|------------|----------------|----------|")
    for alias, profile in report.targets.items():
        name = ", ".join(profile.real_name_candidates) or "Not found"
        loc = profile.location.country or "Unknown"
        lines.append(
            f"| {alias} | {name} | {_format_confidence(profile.aggregate_confidence)} | "
            f"{profile.classification} | {loc} |"
        )
    lines.append("")

    # Target details
    lines.append("## Target Profiles")
    lines.append("")
    for alias, profile in report.targets.items():
        lines.append(_profile_to_markdown(alias, profile))

    # Appendices
    lines.append("## Appendices")
    lines.append("")
    lines.append("### Evidence Summary by Source")
    source_counts: dict[str, int] = {}
    for profile in report.targets.values():
        for e in profile.evidence:
            source_counts[e.source] = source_counts.get(e.source, 0) + 1
    for source, count in sorted(source_counts.items()):
        lines.append(f"- {source}: {count} evidence points")

    lines.append("")
    lines.append("### Methodology Notes")
    lines.append("- Confidence scores use the independence formula: 1 - Π(1 - w×c)")
    lines.append("- Source status entries (weight=0) are excluded from aggregate calculation")
    lines.append("- Real name candidates require confidence > 0.50 from at least one source")
    lines.append("- All profiles are immutable; re-running produces new instances")

    return "\n".join(lines)


def generate_json_report(report: InvestigationReport) -> str:
    """Generate a JSON report string."""
    return json.dumps(report.to_dict(), indent=2, ensure_ascii=False)


def write_report(
    report: InvestigationReport,
    output_dir: str,
    formats: tuple[str, ...] = ("md", "json"),
    base_name: str = "osint_profiling_report",
) -> dict[str, str]:
    """Write report files to disk and return paths."""
    out_path = Path(output_dir)
    out_path.mkdir(parents=True, exist_ok=True)

    written: dict[str, str] = {}

    if "md" in formats:
        md_path = out_path / f"{base_name}.md"
        md_content = generate_markdown_report(report)
        md_path.write_text(md_content, encoding="utf-8")
        written["md"] = str(md_path)
        logger.info("Markdown report written to %s", md_path)

    if "json" in formats:
        json_path = out_path / f"{base_name}.json"
        json_content = generate_json_report(report)
        json_path.write_text(json_content, encoding="utf-8")
        written["json"] = str(json_path)
        logger.info("JSON report written to %s", json_path)

    return written


__all__ = [
    "generate_markdown_report",
    "generate_json_report",
    "write_report",
    "_profile_to_markdown",
]
