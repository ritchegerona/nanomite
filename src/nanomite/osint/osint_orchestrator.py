#!/usr/bin/env python3
"""
Nanomite OSINT Identity Mapping & Doxxing Framework — Main Orchestrator

Authorized security research only. Targets must be owned by the user or
covered by explicit written permission.

This module is target-agnostic: pass any alias and optional real name.
It ingests evidence, merges alias variants, enumerates social profiles,
correlates sources, and writes Markdown + JSON reports.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Optional

# Ensure local package imports work when run as a script
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from osint.identity_mapper import IdentityMapper, EvidenceRecord
from osint.social_enum import SocialEnumerator
from osint.email_harvest import EmailHarvester
from osint.metadata_extractor import MetadataExtractor
from osint.correlator import Correlator
from osint.report_generator import ReportGenerator


def run_osint_orchestrator(
    alias: str,
    real_name: Optional[str] = None,
    output_dir: str = "report",
    extra_evidence: Optional[list[dict]] = None,
    skip_social: bool = False,
    skip_email: bool = False,
) -> dict:
    """
    Run the full OSINT identity-mapping pipeline for a given alias.

    Parameters
    ----------
    alias : str
        The hacker alias/handle to investigate (e.g. "DNH Klammer", "ghostwire").
    real_name : str | None
        Suspected real name, if any (optional).
    output_dir : str
        Directory for generated reports.
    extra_evidence : list[dict] | None
        Additional evidence dicts with keys: source, quote, confidence,
        tags, source_url, date_observed, real_name.
    skip_social : bool
        Skip social media enumeration.
    skip_email : bool
        Skip email harvesting.

    Returns
    -------
    dict
        The correlated identity profile as a dictionary.
    """
    from .agent_integration import run_osint_pipeline

    return run_osint_pipeline(
        alias=alias,
        real_name=real_name,
        output_dir=output_dir,
        extra_evidence=extra_evidence,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="nanomite OSINT orchestrator — identity mapping & doxxing research",
        epilog=(
            "Example: python3 osint_orchestrator.py --alias 'DNH Klammer' "
            "--real-name 'Rodel Lablab' --output report/my_case\n"
        ),
    )
    parser.add_argument(
        "--alias",
        required=True,
        help="Hacker alias, handle, or nickname to investigate",
    )
    parser.add_argument(
        "--real-name",
        default=None,
        help="Suspected real name (optional)",
    )
    parser.add_argument(
        "--output",
        default="report/osint_case",
        help="Output directory prefix for Markdown + JSON reports",
    )
    parser.add_argument(
        "--evidence",
        default=None,
        help="JSON file with extra evidence records (optional)",
    )
    parser.add_argument(
        "--no-social",
        action="store_true",
        help="Skip social media enumeration",
    )
    parser.add_argument(
        "--no-email",
        action="store_true",
        help="Skip email harvesting",
    )
    args = parser.parse_args()

    alias = args.alias.strip()
    real_name = args.real_name.strip() if args.real_name else None
    output_dir = args.output

    extra_evidence = None
    if args.evidence:
        ev_path = Path(args.evidence)
        if ev_path.exists():
            extra_evidence = json.loads(ev_path.read_text(encoding="utf-8"))
        else:
            print(f"[warn] Evidence file not found: {ev_path}", file=sys.stderr)

    print(f"[nanomite OSINT orchestrator] Target alias: {alias}")
    if real_name:
        print(f"[nanomite OSINT orchestrator] Suspected real name: {real_name}")
    print(f"[nanomite OSINT orchestrator] Output directory: {output_dir}")
    print()

    try:
        profile = run_osint_orchestrator(
            alias=alias,
            real_name=real_name,
            output_dir=output_dir,
            extra_evidence=extra_evidence,
            skip_social=args.no_social,
            skip_email=args.no_email,
        )
    except Exception as exc:  # pragma: no cover - runtime safety
        print(f"[nanomite OSINT orchestrator] Pipeline error: {exc}", file=sys.stderr)
        return 1

    print("=" * 60)
    print("OSINT ORCHESTRATION COMPLETE")
    print("=" * 60)
    print(f"Alias: {profile.get('primary_alias')}")
    print(f"Real name: {profile.get('real_name') or '(not provided / unresolved)'}")
    print(f"Confidence: {profile.get('confidence', 0.0):.2f}")
    print(f"Aliases merged: {', '.join(profile.get('aliases', [])) or 'none'}")
    print(f"Evidence items: {len(profile.get('evidence', []))}")
    print(f"Social profiles enumerated: {len(profile.get('social_profiles', []))}")
    print()
    print("Reports written:")
    print(f"  - {output_dir}.md")
    print(f"  - {output_dir}.json")
    print()
    print("Suggested next steps:")
    for step in profile.get("next_steps", [])[:5]:
        print(f"  * {step}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
