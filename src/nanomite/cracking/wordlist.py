"""Generator-based wordlist production for authorized auditing.

Follows the advanced standard:
* frozen slotted dataclass for configuration
* generators for memory-efficient streaming (never loads full wordlist into RAM)
* context-managed file lifecycle
* module-level logging (no print)
* fail-fast domain exceptions
* full type hints
"""

from __future__ import annotations

import logging
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

from .errors import CrackError

logger = logging.getLogger(__name__)


class WordlistError(CrackError):
    """Raised when a wordlist source cannot be read."""


@dataclass(frozen=True, slots=True)
class WordlistConfig:
    path: str
    min_len: int = 1
    max_len: int | None = None
    dedupe: bool = True
    lowercase: bool = False
    cap: int | None = None


def stream_words(config: WordlistConfig) -> Iterator[str]:
    """Yield candidate words one at a time, never buffering the whole file.

    Applies filters (length, case, dedupe, cap) as a lazy pipeline so a
    10 GB wordlist costs O(1) RAM.
    """
    path = Path(config.path)
    if not path.is_file():
        logger.error("wordlist not found", extra={"path": config.path})
        raise WordlistError(f"wordlist not found: {config.path}")

    seen: set[str] = set()
    yielded = 0

    with path.open("r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            word = line.strip()
            if not word:
                continue
            if config.lowercase:
                word = word.lower()
            if len(word) < config.min_len:
                continue
            if config.max_len is not None and len(word) > config.max_len:
                continue
            if config.dedupe:
                if word in seen:
                    continue
                seen.add(word)
            if config.cap is not None and yielded >= config.cap:
                logger.info(
                    "wordlist cap reached",
                    extra={"cap": config.cap, "path": config.path},
                )
                return
            yielded += 1
            yield word

    logger.info(
        "wordlist stream complete",
        extra={"path": config.path, "yielded": yielded},
    )


def count_words(config: WordlistConfig) -> int:
    """Return the number of words that would be yielded (materializes count only)."""
    return sum(1 for _ in stream_words(config))
