"""Core cracking engine: pure logic separated from I/O.

Design
------
* ``CrackSession`` is a context manager. It authorizes on entry, yields the
  session, and cleans up on exit.
* ``attempt()`` is pure: it takes a candidate word and returns a
  ``CrackResult``. Hash comparison is injected so the engine itself never
  touches disk or network.
* All outcomes are typed via ``CrackOutcome`` (an algebraic data type), not
  strings.
* Logging is structured via the standard ``logging`` module; no ``print()``
  appears in library code.
"""

from __future__ import annotations

import hashlib
import logging
import time
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime, timezone
from enum import Enum
from typing import Callable, Final

from .errors import CapabilityInvalid, CrackError, SessionNotEngaged

logger = logging.getLogger(__name__)


class CrackOutcome(str, Enum):
    """Typed outcome of a single cracking attempt."""

    SUCCESS = "success"
    FAILURE = "failure"
    SKIPPED = "skipped"
    OUT_OF_SCOPE = "out_of_scope"


@dataclass(frozen=True, slots=True)
class CrackTarget:
    """A single authorized cracking target.

    ``scope_id`` binds the target to an engagement so a cracked hash can never
    be attributed to an out-of-scope operation.
    """

    path: Final[str]
    hash_type: Final[str]
    hash_value: Final[str]
    scope_id: Final[str]
    max_attempts: Final[int] = 10_000


@dataclass(frozen=True, slots=True)
class CrackResult:
    """Immutable record of one attempt."""

    target_path: Final[str]
    scope_id: Final[str]
    outcome: Final[CrackOutcome]
    plaintext: Final[str | None]
    attempts: Final[int]
    elapsed_ms: Final[int]
    timestamp_utc: Final[str]


class CrackSession:
    """Context-managed cracking session with audit trail.

    Usage
    -----
    >>> target = CrackTarget(
    ...     path="/tmp/test.hash",
    ...     hash_type="sha256",
    ...     hash_value=hashlib.sha256(b"password").hexdigest(),
    ...     scope_id="eng-001",
    ... )
    >>> session = CrackSession(target, capability_token="tok-123")
    >>> with session.engage():
    ...     result = session.attempt("password")
    ...     assert result.outcome is CrackOutcome.SUCCESS
    """

    def __init__(self, target: CrackTarget, capability_token: str) -> None:
        self._target = target
        self._token = capability_token
        self._engaged = False
        self._attempts_used = 0

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    @contextmanager
    def engage(self) -> Iterator[CrackSession]:
        """Authorize on entry, clean up on exit."""

        self._authorize()
        self._engaged = True
        logger.info(
            "crack session engaged",
            extra={
                "target": self._target.path,
                "scope_id": self._target.scope_id,
                "hash_type": self._target.hash_type,
            },
        )
        try:
            yield self
        finally:
            self._engaged = False
            logger.info(
                "crack session disengaged",
                extra={
                    "target": self._target.path,
                    "attempts_used": self._attempts_used,
                },
            )

    def _authorize(self) -> None:
        if not self._token:
            raise CapabilityInvalid("missing capability token")
        if not self._target.scope_id:
            raise CapabilityInvalid("target has no scope_id")

    # ------------------------------------------------------------------
    # Operations
    # ------------------------------------------------------------------

    def attempt(
        self,
        word: str,
        hasher: Callable[[str], str] | None = None,
    ) -> CrackResult:
        """Test one candidate word against the target hash.

        Parameters
        ----------
        word:
            Candidate plaintext to test.
        hasher:
            Optional injected hash function. Defaults to SHA-256. Injection
            keeps this method pure and testable without I/O.

        Returns
        -------
        CrackResult
            Immutable outcome record.
        """

        if not self._engaged:
            raise SessionNotEngaged(
                "session must be engaged via `with session.engage():`"
            )

        if self._attempts_used >= self._target.max_attempts:
            logger.warning(
                "max attempts reached",
                extra={
                    "target": self._target.path,
                    "max_attempts": self._target.max_attempts,
                },
            )
            return CrackResult(
                target_path=self._target.path,
                scope_id=self._target.scope_id,
                outcome=CrackOutcome.SKIPPED,
                plaintext=None,
                attempts=0,
                elapsed_ms=0,
                timestamp_utc=_now_iso(),
            )

        start = time.perf_counter()
        hash_fn = hasher or _default_hasher(self._target.hash_type)
        digest = hash_fn(word)
        matched = _constant_time_compare(digest, self._target.hash_value)
        elapsed_ms = int((time.perf_counter() - start) * 1000)
        self._attempts_used += 1

        outcome: CrackOutcome = CrackOutcome.SUCCESS if matched else CrackOutcome.FAILURE

        logger.debug(
            "crack attempt",
            extra={
                "word": word,
                "outcome": outcome.value,
                "elapsed_ms": elapsed_ms,
                "attempt": self._attempts_used,
            },
        )

        return CrackResult(
            target_path=self._target.path,
            scope_id=self._target.scope_id,
            outcome=outcome,
            plaintext=word if matched else None,
            attempts=self._attempts_used,
            elapsed_ms=elapsed_ms,
            timestamp_utc=_now_iso(),
        )

    @property
    def attempts_used(self) -> int:
        return self._attempts_used


# -----------------------------------------------------------------------
# Pure helpers (no I/O, no state)
# -----------------------------------------------------------------------

_SUPPORTED: Final[frozenset[str]] = frozenset(
    {"md5", "sha1", "sha224", "sha256", "sha384", "sha512"}
)


def _default_hasher(hash_type: str) -> Callable[[str], str]:
    algorithm = hash_type.lower()
    if algorithm not in _SUPPORTED:
        msg = f"unsupported hash type: {hash_type}"
        raise HashUnsupported(msg)
    return lambda word: hashlib.new(algorithm, word.encode("utf-8")).hexdigest()


def _constant_time_compare(a: str, b: str) -> bool:
    ba = a.encode("utf-8")
    bb = b.encode("utf-8")
    if len(ba) != len(bb):
        return False
    result = 0
    for x, y in zip(ba, bb):
        result |= x ^ y
    return result == 0


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


class HashUnsupported(CrackError):
    """Raised when the hash algorithm is not supported by this engine."""
