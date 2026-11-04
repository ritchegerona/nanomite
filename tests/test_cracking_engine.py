"""Tests for the advanced cracking engine.

Invalid states (disengaged session, max attempts, unsupported hash) must
raise or return typed outcomes. Valid transitions must succeed.
"""

from __future__ import annotations

import hashlib

import pytest

from nanomite.cracking.engine import (
    CrackOutcome,
    CrackResult,
    CrackSession,
    CrackTarget,
)
from nanomite.cracking.errors import (
    CapabilityInvalid,
    CrackError,
    SessionNotEngaged,
)


def _make_target(
    plaintext: str = "password",
    hash_type: str = "sha256",
    scope_id: str = "eng-001",
) -> CrackTarget:
    digest = hashlib.new(hash_type, plaintext.encode("utf-8")).hexdigest()
    return CrackTarget(
        path="/tmp/test.hash",
        hash_type=hash_type,
        hash_value=digest,
        scope_id=scope_id,
    )


def test_attempt_requires_engaged_session() -> None:
    session = CrackSession(_make_target(), capability_token="tok-123")
    with pytest.raises(SessionNotEngaged):
        session.attempt("password")


def test_missing_capability_token_raises() -> None:
    target = _make_target()
    session = CrackSession(target, capability_token="")
    with pytest.raises(CapabilityInvalid), session.engage():
        session.attempt("password")


def test_missing_scope_id_raises() -> None:
    target = CrackTarget(
        path="/tmp/test.hash",
        hash_type="sha256",
        hash_value="x",
        scope_id="",
    )
    session = CrackSession(target, capability_token="tok-123")
    with pytest.raises(CapabilityInvalid), session.engage():
        session.attempt("password")


def test_successful_attempt_returns_success() -> None:
    session = CrackSession(_make_target("password"), capability_token="tok-123")
    with session.engage():
        result = session.attempt("password")
        assert result.outcome is CrackOutcome.SUCCESS
        assert result.plaintext == "password"
        assert result.attempts == 1
        assert result.elapsed_ms >= 0


def test_failed_attempt_returns_failure() -> None:
    session = CrackSession(_make_target("password"), capability_token="tok-123")
    with session.engage():
        result = session.attempt("wrong")
        assert result.outcome is CrackOutcome.FAILURE
        assert result.plaintext is None
        assert result.attempts == 1


def test_attempts_are_counted() -> None:
    session = CrackSession(_make_target("password"), capability_token="tok-123")
    with session.engage():
        session.attempt("a")
        session.attempt("b")
        assert session.attempts_used == 2


def test_max_attempts_returns_skipped() -> None:
    target = _make_target(scope_id="eng-001")
    target = CrackTarget(
        path=target.path,
        hash_type=target.hash_type,
        hash_value=target.hash_value,
        scope_id=target.scope_id,
        max_attempts=0,
    )
    session = CrackSession(target, capability_token="tok-123")
    with session.engage():
        result = session.attempt("password")
        assert result.outcome is CrackOutcome.SKIPPED
        assert result.attempts == 0


def test_unsupported_hash_raises() -> None:
    target = CrackTarget(
        path="/tmp/test.hash",
        hash_type="bogus-algo",
        hash_value="x",
        scope_id="eng-001",
    )
    session = CrackSession(target, capability_token="tok-123")
    with session.engage(), pytest.raises(CrackError):
        session.attempt("password")


def test_injected_hasher_works() -> None:
    target = CrackTarget(
        path="/tmp/test.hash",
        hash_type="sha256",
        hash_value=hashlib.sha256(b"secret").hexdigest(),
        scope_id="eng-001",
    )
    session = CrackSession(target, capability_token="tok-123")
    with session.engage():
        result = session.attempt(
            "secret", hasher=lambda w: hashlib.sha256(w.encode()).hexdigest()
        )
        assert result.outcome is CrackOutcome.SUCCESS
        assert result.plaintext == "secret"


def test_result_is_immutable() -> None:
    result = CrackResult(
        target_path="/tmp/x",
        scope_id="eng-001",
        outcome=CrackOutcome.FAILURE,
        plaintext=None,
        attempts=1,
        elapsed_ms=5,
        timestamp_utc="2024-01-01T00:00:00+00:00",
    )
    with pytest.raises(AttributeError):
        result.attempts = 99  # type: ignore[misc]


def test_all_outcomes_are_typed() -> None:
    assert CrackOutcome.SUCCESS.value == "success"
    assert CrackOutcome.FAILURE.value == "failure"
    assert CrackOutcome.SKIPPED.value == "skipped"
    assert CrackOutcome.OUT_OF_SCOPE.value == "out_of_scope"
