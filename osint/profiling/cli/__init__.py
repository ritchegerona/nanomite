"""CLI for OSINT Profiling Framework."""

from __future__ import annotations
import argparse
import asyncio
import logging
import sys
from pathlib import Path

from ..config import load_targets_from_json
from ..detector import TargetDetector
from ..engine import run_profiling
from ..pipeline import run_pipeline
from ..reporter import generate_report

logger = logging.getLogger(__name__)


def setup_logging(verbose: bool = False) -> None:
    """Configure logging."""
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
        datefmt="%H:%M:%S",
    )


def create_parser() -> argparse.ArgumentParser:
    """Create argument parser."""
    parser = argparse.ArgumentParser(
        prog="osint-profiler",
        description="Auto-detecting OSINT profiling framework for identity resolution",
    )
    parser.add_argument("-v", "--verbose", action="store_true", help="Verbose logging")
    parser.add_argument("--workspace", default="/Users/ritchegerona/nanomite", help="Workspace root")
    parser.add_argument("--targets-json", default="osint/profiling/targets.json", help="Seed registry")
    parser.add_argument("--output-dir", default="osint/profiling/reports", help="Output directory")
    parser.add_argument("--skip-sources", nargs="*", default=["common_crawl"], help="Sources to skip")
    parser.add_argument("--timeout", type=float, default=30.0, help="Source timeout (seconds)")
    
    subparsers = parser.add_subparsers(dest="command", required=True)
    
    # profile command
    profile_parser = subparsers.add_parser("profile", help="Profile a specific target")
    profile_parser.add_argument("alias", help="Target alias to profile")
    profile_parser.add_argument("--format", choices=["markdown", "json"], default="markdown")
    profile_parser.add_argument("--output", help="Output file (default: stdout)")
    
    # auto-detect command
    detect_parser = subparsers.add_parser("detect", help="Auto-detect targets from workspace")
    detect_parser.add_argument("--save", action="store_true", help="Save detected targets to JSON")
    
    # auto-execute command
    exec_parser = subparsers.add_parser("auto-execute", help="Run full auto-detect -> profile -> report pipeline")
    exec_parser.add_argument("--no-common-crawl", action="store_true", help="Skip Common Crawl (slow)")
    
    # list command
    list_parser = subparsers.add_parser("list", help="List known targets")
    
    return parser


async def cmd_profile(args: argparse.Namespace) -> int:
    """Profile a single target."""
    # Load targets
    targets_path = Path(args.workspace) / args.targets_json
    if not targets_path.exists():
        logger.error("Targets file not found: %s", targets_path)
        return 1
    
    targets = load_targets_from_json(targets_path)
    
    if args.alias not in targets:
        logger.error("Target not found: %s", args.alias)
        logger.info("Available targets: %s", ", ".join(targets.keys()))
        return 1
    
    target = targets[args.alias]
    
    # Run profiling
    profile = await run_profiling(
        target=target,
        skip_sources=args.skip_sources,
        timeout=args.timeout,
    )
    
    # Generate report
    if args.output:
        output_path = Path(args.output)
    else:
        output_path = Path(args.workspace) / args.output_dir / args.alias / f"profile.{args.format}"
    
    generate_report(profile, output_path, format=args.format)
    logger.info("Report saved to %s", output_path)
    
    # Print summary to stdout
    print(f"Target: {profile.target.alias}")
    print(f"Tier: {profile.tier.value}")
    print(f"Confidence: {profile.aggregate_confidence:.2f}")
    print(f"Real name candidates: {profile.real_name_candidates or 'None'}")
    print(f"Location: {profile.location_profile.get('likely_country', 'Unknown')}")
    print(f"Evidence items: {len(profile.evidence)}")
    
    return 0


async def cmd_detect(args: argparse.Namespace) -> int:
    """Auto-detect targets."""
    detector = TargetDetector(args.workspace)
    
    # Load from JSON
    targets_path = Path(args.workspace) / args.targets_json
    if targets_path.exists():
        detector.detect_from_targets_json(targets_path)
    
    # Scan workspace
    detector.detect_from_workspace_files()
    
    # Check memory
    detector.detect_from_memory()
    
    targets = detector.get_all_targets()
    
    print(f"Detected {len(targets)} targets:")
    for alias, target in targets.items():
        sources = target.metadata.get("detected_from", "unknown")
        print(f"  - {alias} (from {sources})")
    
    if args.save:
        detector.save_targets(targets_path)
        print(f"Saved to {targets_path}")
    
    return 0


async def cmd_auto_execute(args: argparse.Namespace) -> int:
    """Run full pipeline."""
    skip_sources = args.skip_sources or []
    if args.no_common_crawl:
        skip_sources.append("common_crawl")
    
    profiles = await run_pipeline(
        workspace_root=args.workspace,
        output_dir=args.output_dir,
        targets_json=args.targets_json,
        skip_sources=skip_sources,
        timeout=args.timeout,
    )
    
    print(f"\nPipeline complete. Profiled {len(profiles)} targets.")
    print(f"Reports saved to: {Path(args.workspace) / args.output_dir}")
    
    # Print summary
    tiers = {}
    for alias, profile in profiles.items():
        tier = profile.tier.value
        tiers[tier] = tiers.get(tier, 0) + 1
    
    for tier, count in sorted(tiers.items()):
        print(f"  {tier}: {count}")
    
    return 0


async def cmd_list(args: argparse.Namespace) -> int:
    """List known targets."""
    targets_path = Path(args.workspace) / args.targets_json
    if not targets_path.exists():
        logger.error("Targets file not found: %s", targets_path)
        return 1
    
    targets = load_targets_from_json(targets_path)
    
    print(f"Known targets ({len(targets)}):")
    for alias, target in targets.items():
        print(f"  - {alias}")
        if target.real_name_candidates:
            print(f"    Real name candidates: {target.real_name_candidates}")
        if target.telegram_usernames:
            print(f"    Telegram: {target.telegram_usernames}")
        if target.facebook_ids:
            print(f"    Facebook IDs: {target.facebook_ids}")
        if target.instagram_usernames:
            print(f"    Instagram: {target.instagram_usernames}")
    
    return 0


async def main() -> int:
    """Main CLI entry point."""
    parser = create_parser()
    args = parser.parse_args()
    
    setup_logging(args.verbose)
    
    commands = {
        "profile": cmd_profile,
        "detect": cmd_detect,
        "auto-execute": cmd_auto_execute,
        "list": cmd_list,
    }
    
    if args.command not in commands:
        parser.print_help()
        return 1
    
    try:
        return await commands[args.command](args)
    except KeyboardInterrupt:
        logger.info("Interrupted")
        return 130
    except Exception as e:
        logger.exception("Command failed: %s", e)
        return 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))