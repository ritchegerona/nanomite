"""Evidence-chain report generation for OSINT investigations.

Produces Markdown and JSON reports documenting the alias-to-identity
linkage with source citations, confidence scores, and suggested next
steps. Suitable for authorized security research and CTIA reporting.
"""

from __future__ import annotations

from dataclasses import asdict
from pathlib import Path
from typing import Optional

from .correlator import IdentityProfile


class ReportGenerator:
    """Generate investigation reports from correlated identity profiles."""

    def __init__(self, output_dir: str = "report"):
        self.output_dir = Path(output_dir)
        self.output_dir.mkdir(parents=True, exist_ok=True)

    def generate_markdown(self, profile: IdentityProfile, filename: Optional[str] = None) -> str:
        lines = []
        lines.append(f"# OSINT Identity Investigation Report")
        lines.append(f"## Primary Alias: {profile.primary_alias}")
        lines.append("")
        lines.append(f"**Real Name:** {profile.real_name or 'Not confirmed'}")
        lines.append(f"**Overall Confidence:** {profile.confidence:.2f} / 1.00")
        lines.append(f"**Evidence Items:** {profile.evidence_count}")
        lines.append("")

        if profile.aliases:
            lines.append("## Known Aliases")
            for a in profile.aliases:
                lines.append(f"- {a}")
            lines.append("")

        if profile.affiliations:
            lines.append("## Affiliations")
            for a in profile.affiliations:
                lines.append(f"- {a}")
            lines.append("")

        if profile.possible_employers:
            lines.append("## Possible Employers / Organizations")
            for e in profile.possible_employers:
                lines.append(f"- {e}")
            lines.append("")

        lines.append("## Social Profiles")
        if profile.social_profiles:
            for sp in profile.social_profiles:
                lines.append(f"- **{sp.get('platform', '?')}**: {sp.get('username', '?')} — {sp.get('url', '')}")
        else:
            lines.append("- None enumerated")
        lines.append("")

        lines.append("## Email Addresses")
        if profile.emails:
            for em in profile.emails:
                lines.append(f"- {em.get('email', '?')} (source: {em.get('source', '?')})")
        else:
            lines.append("- None harvested")
        lines.append("")

        lines.append("## Evidence Chain")
        if profile.evidence:
            for i, ev in enumerate(profile.evidence, 1):
                lines.append(f"### Evidence {i}")
                lines.append(f"- **Source:** {ev.get('source', '?')}")
                if ev.get('source_url'):
                    lines.append(f"- **URL:** {ev['source_url']}")
                lines.append(f"- **Confidence:** {ev.get('confidence', 0):.2f}")
                lines.append(f"- **Quote:** {ev.get('quote', '')}")
                lines.append("")
        else:
            lines.append("- None recorded")
            lines.append("")

        lines.append("## Metadata Findings")
        if profile.metadata_findings:
            for mf in profile.metadata_findings:
                lines.append(f"- {mf}")
        else:
            lines.append("- None")
        lines.append("")

        lines.append("## Suggested Next Steps")
        lines.append("1. Verify social profile ownership via direct platform inspection.")
        lines.append("2. Cross-reference possible employers with public registries.")
        lines.append("3. Monitor affiliated group channels for additional self-doxxing.")
        lines.append("4. Request public records through authorized legal channels where applicable.")
        lines.append("")

        content = "\n".join(lines)

        if filename is None:
            safe = profile.primary_alias.replace(" ", "_").replace("/", "_")
            filename = f"osint_report_{safe}.md"

        out_path = self.output_dir / filename
        out_path.write_text(content, encoding="utf-8")
        return str(out_path)

    def generate_json(self, profile: IdentityProfile, filename: Optional[str] = None) -> str:
        if filename is None:
            safe = profile.primary_alias.replace(" ", "_").replace("/", "_")
            filename = f"osint_profile_{safe}.json"
        out_path = self.output_dir / filename
        out_path.write_text(__import__("json").dumps(profile.to_dict(), indent=2), encoding="utf-8")
        return str(out_path)