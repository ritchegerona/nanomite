"""OSINT auto-trigger integration for nanomite agent.

Provides:
- OsintTrigger: keyword detection for OSINT/doxxing/identity tasks
- run_osint_pipeline: executes the full orchestrator when triggered
- is_osint_task: convenience boolean check
- auto_run: automatic entry point for the agent loop (detects + runs)

Authorized security research only.
"""

from __future__ import annotations

from dataclasses import asdict
from pathlib import Path
from typing import Optional

from .osint_trigger import OsintTrigger, OsintTriggerResult
from .identity_mapper import IdentityMapper, EvidenceRecord
from .social_enum import SocialEnumerator
from .email_harvest import EmailHarvester
from .metadata_extractor import MetadataExtractor
from .correlator import Correlator
from .report_generator import ReportGenerator


def is_osint_task(text: str) -> bool:
    """Return True if ``text`` looks like an authorized OSINT/doxxing task."""
    return OsintTrigger().scan(text).triggered


def scan_for_osint(text: str) -> OsintTriggerResult:
    """Scan ``text`` and return the full trigger result."""
    return OsintTrigger().scan(text)


def run_osint_pipeline(
    alias: str,
    real_name: Optional[str] = None,
    output_dir: str = "report",
    extra_evidence: Optional[list[dict]] = None,
    skip_social: bool = False,
    skip_email: bool = False,
) -> dict:
    """Run the full OSINT identity-mapping pipeline for a given alias.

    Parameters
    ----------
    alias:
        The hacker alias/handle to investigate (e.g. "DNH Klammer").
    real_name:
        Known real name, if any (optional).
    output_dir:
        Directory for generated reports.
    extra_evidence:
        Additional evidence dicts with keys: source, quote, confidence,
        tags, source_url, date_observed, real_name.
    skip_social:
        Skip social media enumeration.
    skip_email:
        Skip email harvesting.

    Returns
    -------
    dict
        The correlated identity profile as a dictionary.
    """
    mapper = IdentityMapper()
    correlator = Correlator(mapper)
    reporter = ReportGenerator(output_dir=output_dir)

    # Create candidate for the alias
    cand = mapper.create_candidate(alias, real_name=real_name)

    # Add any extra evidence passed in
    if extra_evidence:
        for ev in extra_evidence:
            mapper.add_evidence(
                alias=alias,
                source=ev.get("source", "user-provided"),
                quote=ev.get("quote", ""),
                confidence=ev.get("confidence", 0.5),
                tags=ev.get("tags", []),
                source_url=ev.get("source_url"),
                date_observed=ev.get("date_observed"),
                real_name=ev.get("real_name") or real_name,
            )

    # Merge common alias variants if they exist as separate candidates
    variants = [
        alias.replace(" ", "."),
        alias.replace(" ", ""),
        alias.replace(" ", "") + "v1",
        alias.replace(" ", "") + "V2",
        alias.replace(" ", "_"),
    ]
    for variant in variants:
        if variant == alias:
            continue  # don't merge alias with itself
        vc = mapper.get_candidate(variant)
        if vc:
            mapper.merge_alias(alias, variant)

    # Social enumeration
    social_profiles: list[dict] = []
    if not skip_social:
        enumerator = SocialEnumerator()
        social = enumerator.enumerate(alias)
        if real_name:
            social.extend(enumerator.enumerate(real_name))

        # Deduplicate social profiles
        seen = set()
        for sp in social:
            d = sp.to_dict() if hasattr(sp, "to_dict") else sp
            key = (d.get("platform"), d.get("username"))
            if key not in seen:
                seen.add(key)
                social_profiles.append(d)

    # Email harvesting placeholder (extend as needed)
    emails: list[dict] = []
    if not skip_email:
        # EmailHarvester can be wired here to scan harvested pages
        pass

    # Build profile
    profile = correlator.build_profile(
        alias=alias,
        social_profiles=social_profiles,
        emails=emails,
        metadata_findings=[],
    )
    profile.confidence = correlator.score_confidence(profile)

    # Generate reports
    md_path = reporter.generate_markdown(profile)
    json_path = reporter.generate_json(profile)
    mapper.export_json(str(Path(output_dir) / f"{alias.replace(' ', '_')}_mapper_export.json"))

    result = asdict(profile)
    result["report_paths"] = {"markdown": md_path, "json": json_path}
    return result


def auto_run(text: str, output_dir: str = "report") -> Optional[dict]:
    """Automatic entry point for the agent loop.

    Scans raw user input for OSINT intent. If triggered, extracts the
    primary alias and runs the full pipeline automatically.

    Parameters
    ----------
    text : str
        Raw user input.
    output_dir : str
        Directory for generated reports.

    Returns
    -------
    dict | None
        The profile dict if OSINT was triggered and ran; None otherwise.
    """
    result = scan_for_osint(text)
    if not result.triggered:
        return None

    aliases = result.extracted_aliases
    if not aliases:
        return None

    # Take the first extracted alias as the primary target
    primary_alias = aliases[0]

    # Heuristic: if there are two capitalized multiword phrases, the first
    # is likely the alias and the second is the suspected real name.
    # Otherwise look for a quoted phrase that looks like a person name.
    real_name: Optional[str] = None
    if len(aliases) >= 2:
        candidates = [a for a in aliases[1:] if len(a.split()) >= 2]
        if candidates:
            real_name = candidates[0]
        else:
            real_name = aliases[1]

    return run_osint_pipeline(
        alias=primary_alias,
        real_name=real_name,
        output_dir=output_dir,
    )


__all__ = [
    "is_osint_task",
    "scan_for_osint",
    "run_osint_pipeline",
    "auto_run",
    "OsintTrigger",
    "OsintTriggerResult",
]
