"""Command-line interface for osint_profiler.

Argparse-based CLI per DEVELOPMENT_STANDARDS.md.
Supports single alias, multiple aliases, and batch profiling from file.
"""

from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path
from typing import Sequence

from ..config.settings import get_settings
from ..core.engine import ProfilingEngine
from ..core.models import InvestigationReport
from ..reporter import write_report
from ..sources import (
    cc_gather,
    crt_sh_gather,
    darkweb_gather,
    facebook_gather,
    instagram_gather,
    telegram_gather,
    wayback_gather,
    whois_gather,
)

logger = logging.getLogger(__name__)


def build_parser() -> argparse.ArgumentParser:
    """Build and return the argument parser."""
    parser = argparse.ArgumentParser(
        prog="osint_profiler",
        description="Advanced OSINT profiling and identity resolution framework",
    )

    parser.add_argument(
        "aliases",
        nargs="*",
        metavar="ALIAS",
        help="Target alias(es) to profile (e.g., siningangmixwithgabi happygoluckyph)",
    )

    parser.add_argument(
        "--file",
        "-f",
        metavar="PATH",
        help="Path to file containing one alias per line",
    )

    parser.add_argument(
        "--output-dir",
        "-o",
        metavar="DIR",
        default=None,
        help="Output directory for reports (default: config output_dir)",
    )

    parser.add_argument(
        "--format",
        choices=["md", "json", "both"],
        default="both",
        help="Report output format (default: both)",
    )

    parser.add_argument(
        "--base-name",
        metavar="NAME",
        default="osint_profiling_report",
        help="Base filename for output reports",
    )

    parser.add_argument(
        "--allow-active",
        action="store_true",
        default=False,
        help="Enable active reconnaissance (requires authorization and tooling)",
    )

    parser.add_argument(
        "--no-report",
        action="store_true",
        default=False,
        help="Skip report generation; print summary to stdout only",
    )

    parser.add_argument(
        "--verbose",
        "-v",
        action="count",
        default=0,
        help="Increase verbosity (-v for INFO, -vv for DEBUG)",
    )

    return parser


def _read_aliases_from_file(path: str) -> list[str]:
    """Read aliases from a file, one per line, ignoring blanks and comments."""
    aliases: list[str] = []
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            aliases.append(line)
    return aliases


def _collect_aliases(cli_aliases: Sequence[str], file_path: str | None) -> list[str]:
    """Collect aliases from CLI arguments and optional file."""
    aliases = list(cli_aliases)
    if file_path:
        aliases.extend(_read_aliases_from_file(file_path))
    # Deduplicate while preserving order
    seen: set[str] = set()
    unique: list[str] = []
    for a in aliases:
        if a not in seen:
            seen.add(a)
            unique.append(a)
    return unique


def _build_engine(allow_active: bool) -> ProfilingEngine:
    """Build a ProfilingEngine with all passive sources registered."""
    engine = ProfilingEngine(allow_active=allow_active)

    # Register passive sources
    engine.register_source("facebook", facebook_gather)
    engine.register_source("instagram", instagram_gather)
    engine.register_source("telegram", telegram_gather)
    engine.register_source("whois", whois_gather)
    engine.register_source("crt_sh", crt_sh_gather)
    engine.register_source("wayback", wayback_gather)
    engine.register_source("common_crawl", cc_gather)

    # Register active sources only if allowed
    if allow_active:
        engine.register_source("darkweb", darkweb_gather)

    return engine


def main(argv: Sequence[str] | None = None) -> int:
    """CLI entry point. Returns process exit code."""
    parser = build_parser()
    args = parser.parse_args(argv)

    # Configure logging
    log_level = logging.WARNING
    if args.verbose == 1:
        log_level = logging.INFO
    elif args.verbose >= 2:
        log_level = logging.DEBUG
    logging.basicConfig(
        level=log_level,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    settings = get_settings()

    # Collect aliases
    aliases = _collect_aliases(args.aliases, args.file)
    if not aliases:
        parser.error("At least one alias is required (positional ARG or --file)")

    logger.info("Profiling %d alias(es): %s", len(aliases), ", ".join(aliases))

    # Build engine and run
    engine = _build_engine(args.allow_active)
    report = engine.profile_multiple(aliases)

    # Print summary to stdout
    print(f"\n{'='*60}")
    print(f"OSINT Profiling Complete")
    print(f"{'='*60}")
    print(f"Total targets:  {report.total_targets}")
    print(f"Confirmed:      {report.confirmed_count}")
    print(f"Unresolved:     {report.unresolved_count}")
    print(f"{'='*60}\n")

    for alias, profile in report.targets.items():
        name = ", ".join(profile.real_name_candidates) or "Not found"
        print(
            f"  {alias:<30} | {name:<35} | "
            f"{profile.aggregate_confidence:.2f} | {profile.classification}"
        )
    print()

    # Write reports
    if not args.no_report:
        output_dir = args.output_dir or settings.output_dir
        formats = ("md", "json") if args.format == "both" else (args.format,)
        written = write_report(
            report,
            output_dir=output_dir,
            formats=formats,
            base_name=args.base_name,
        )
        for fmt, path in written.items():
            print(f"Report ({fmt}): {path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
