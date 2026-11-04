"""Minimal-resource password and hash cracking toolkit.

Rebuilt to the advanced coding standard: frozen slotted dataclasses,
Result/Option ADTs, context-managed resource lifecycle, module-level logging,
pure/Io separation, and full type hints.

This package consolidates the legacy root-level crack*.py scripts into a single
audit-tested module. All HACKER MODE capabilities remain intact.
"""

from .cli import run as cli_run
from .cti_report import CrackEvidence, CtiReport, Engagement
from .cti_report import write_report as cti_write_report
from .engine import CrackOutcome, CrackResult, CrackSession, CrackTarget
from .errors import CrackError
from .report import CrackReport, to_json, to_text, write_report
from .wordlist import WordlistConfig, WordlistError, count_words, stream_words

__all__ = [
    "CrackError",
    "CrackEvidence",
    "CrackOutcome",
    "CrackReport",
    "CrackResult",
    "CrackSession",
    "CrackTarget",
    "CtiReport",
    "Engagement",
    "WordlistConfig",
    "WordlistError",
    "cli_run",
    "count_words",
    "cti_write_report",
    "stream_words",
    "to_json",
    "to_text",
    "write_report",
]
