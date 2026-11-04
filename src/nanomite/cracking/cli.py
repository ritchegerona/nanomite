"""Command-line interface for the cracking engine.

Follows the advanced standard:
* argparse with explicit subcommands and exit codes
* module-level logging (no print in library code)
* pure logic separated from I/O
* frozen dataclasses for configuration
* fail-fast domain exceptions
"""

from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

from .engine import CrackOutcome, CrackSession, CrackTarget
from .errors import CapabilityInvalid, CrackError, SessionNotEngaged

logger = logging.getLogger("nanomite.cracking.cli")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="nanomite-crack",
        description="Authorized offline hash-strength audit (HACKER MODE).",
    )
    parser.add_argument(
        "--scope-id",
        required=True,
        help="Engagement scope identifier (binds results to an assessment).",
    )
    parser.add_argument(
        "--capability-token",
        required=True,
        help="Unforgeable capability token issued at activation time.",
    )
    parser.add_argument(
        "--hash-type",
        default="sha256",
        choices=["md5", "sha1", "sha224", "sha256", "sha384", "sha512"],
        help="Hash algorithm to audit (default: sha256).",
    )
    parser.add_argument(
        "--max-attempts",
        type=int,
        default=10_000,
        help="Hard cap on guesses per target (default: 10000).",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Emit debug-level structured logs.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    single = sub.add_parser("single", help="Audit one hash target")
    single.add_argument("hash_value", help="The hash digest to test")
    single.add_argument("wordlist", help="File with one candidate per line")
    single.add_argument(
        "--target-path",
        default="-",
        help="Logical path for audit records (default: '-').",
    )

    return parser


def _read_wordlist(path: str) -> list[str]:
    p = Path(path)
    if not p.is_file():
        logger.error("wordlist not found", extra={"path": path})
        raise SystemExit(2)
    with p.open("r", encoding="utf-8", errors="replace") as fh:
        return [line.strip() for line in fh if line.strip()]


def run(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )

    if args.command != "single":
        logger.error("unknown command", extra={"command": args.command})
        return 2

    words = _read_wordlist(args.wordlist)

    target = CrackTarget(
        path=args.target_path,
        hash_type=args.hash_type,
        hash_value=args.hash_value,
        scope_id=args.scope_id,
        max_attempts=args.max_attempts,
    )

    session = CrackSession(target, capability_token=args.capability_token)

    try:
        with session.engage():
            for word in words:
                result = session.attempt(word)
                if result.outcome is CrackOutcome.SUCCESS:
                    logger.info(
                        "hash cracked",
                        extra={
                            "plaintext": result.plaintext,
                            "attempts": result.attempts,
                            "scope_id": result.scope_id,
                        },
                    )
                    print(result.plaintext or "")
                    return 0
                logger.debug(
                    "attempt failed",
                    extra={"word": word, "attempt": result.attempts},
                )
    except SessionNotEngaged as exc:
        logger.error("session error", extra={"error": str(exc)})
        return 3
    except CapabilityInvalid as exc:
        logger.error("capability invalid", extra={"error": str(exc)})
        return 4
    except CrackError as exc:
        logger.error("crack error", extra={"error": str(exc)})
        return 5

    logger.info(
        "audit complete, no match",
        extra={
            "attempts_used": session.attempts_used,
            "scope_id": args.scope_id,
        },
    )
    return 1


def main() -> None:
    sys.exit(run(sys.argv[1:]))


if __name__ == "__main__":
    main()
