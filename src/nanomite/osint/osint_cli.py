#!/usr/bin/env python3
"""
OSINT Identity Mapping CLI — Generic target-agnostic entry point.

Usage:
    python3 osint_cli.py --alias "HackerHandle" --real-name "John Doe"
    python3 osint_cli.py --alias "DNH Klammer" --real-name "Rodel Lablab" --output report/my_case
    python3 osint_cli.py --alias "ghostwire" --output report/ghostwire_case

This runs the full nanomite OSINT pipeline:
  1. Alias ingestion & normalization
  2. Cross-platform social enumeration
  3. Evidence ingestion & confidence scoring
  4. Correlation & report generation (Markdown + JSON)

Works for any alias/target you supply. No hardcoded names.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Allow running both as a module and as a standalone script
try:
    from src.nanomite.osint.agent_integration import run_osint_pipeline, scan_for_osint
    from src.nanomite.osint.identity_mapper import IdentityMapper
except ImportError:  # pragma: no cover - fallback for direct execution
    sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
    from src.nanomite.osint.agent_integration import run_osint_pipeline, scan_for_osint  # type: ignore
    from src.nanomite.osint.identity_mapper import IdentityMapper  # type: ignore


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="nanomite OSINT identity mapping & doxxing research pipeline",
        epilog="Example: python3 osint_cli.py --alias \"DNH Klammer\" --real-name \"Rodel Lablab\" --output report/my_case",
    )
    parser.add_argument(
        "--alias",
        required=True,
        help="Hacker alias, handle, or nickname to investigate (e.g. 'DNH Klammer', 'ghostwire')",
    )
    parser.add_argument(
        "--real-name",
        default=None,
        help="Claimed or suspected real name (optional; pipeline will still run without it)",
    )
    parser.add_argument(
        "--output",
        default="report/osint_case",
        help="Output directory prefix for Markdown + JSON reports (default: report/osint_case)",
    )
    parser.add_argument(
        "--no-social",
        action="store_true",
        help="Skip social media enumeration (faster, fewer external calls)",
    )
    parser.add_argument(
        "--no-email",
        action="store_true",
        help="Skip email harvesting from harvested web pages",
    )
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    alias: str = args.alias.strip()
    real_name: str | None = args.real_name.strip() if args.real_name else None
    output_dir: str = args.output

    print(f"[nanomite OSINT] Target alias: {alias}")
    if real_name:
        print(f"[nanomite OSINT] Suspected real name: {real_name}")
    print(f"[nanomite OSINT] Output directory: {output_dir}")
    print()

    # Sanity check: confirm the trigger logic sees this as an OSINT task
    trigger_test = scan_for_osint(f"find the real identity of {alias}")
    print(f"[nanomite OSINT] OSINT task detected: {trigger_test}")
    print()

    # Run the full pipeline
    try:
        profile = run_osint_pipeline(
            alias=alias,
            real_name=real_name,
            output_dir=output_dir,
        )
    except Exception as exc:  # pragma: no cover - runtime safety
        print(f"[nanomite OSINT] Pipeline error: {exc}", file=sys.stderr)
        return 1

    # Print concise summary to stdout
    print("=" * 60)
    print("OSINT PIPELINE COMPLETE")
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
