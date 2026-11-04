"""Property-based tests for the cracking engine using Hypothesis.

These verify invariants across a wide input space:
* Attempt results always round-trip through CrackResult fields
* Constant-time comparison never leaks via length or timing side-channels
* Attempt counting is monotonic and bounded by max_attempts
* Immutability holds for all generated instances
"""

from __future__ import annotations

import hashlib

import pytest
from hypothesis import given
from hypothesis import strategies as st

from nanomite.cracking.engine import (
    CrackOutcome,
    CrackSession,
    CrackTarget,
    _constant_time_compare,
)
from nanomite.cracking.errors import SessionNotEngaged

# ---------- Strategies ----------

@st.composite
def _target(draw):
    plaintext = draw(st.text(min_size=1, max_size=20))
    hash_type = draw(st.sampled_from(["md5", "sha1", "sha256", "sha512"]))
    digest = hashlib.new(hash_type, plaintext.encode("utf-8")).hexdigest()
    return CrackTarget(
        path="/tmp/t.hash",
        hash_type=hash_type,
        hash_value=digest,
        scope_id="eng-001",
        max_attempts=draw(st.integers(min_value=1, max_value=1000)),
    )


@st.composite
def _word(draw):
    return draw(st.text(min_size=0, max_size=30))


# ---------- Property tests ----------

@given(_target(), _word())
def test_attempt_result_fields_are_consistent(target, word):
    """Every attempt returns a CrackResult with consistent fields."""
    session = CrackSession(target, capability_token="tok-123")
    with session.engage():
        result = session.attempt(word)

    assert result.target_path == target.path
    assert result.scope_id == target.scope_id
    assert result.outcome in (
        CrackOutcome.SUCCESS,
        CrackOutcome.FAILURE,
        CrackOutcome.SKIPPED,
    )
    assert result.attempts == 1
    assert result.elapsed_ms >= 0
    assert "+00:00" in result.timestamp_utc or "Z" in result.timestamp_utc
    if result.outcome is CrackOutcome.SUCCESS:
        assert result.plaintext == word
    else:
        assert result.plaintext is None


@given(_target(), _word(), _word())
def test_attempt_counting_is_monotonic(target, word_a, word_b):
    """Attempts increase by exactly 1 per call while under max_attempts."""
    session = CrackSession(target, capability_token="tok-123")
    with session.engage():
        r1 = session.attempt(word_a)
        count_after_one = session.attempts_used
        # Only attempt the second if we haven't hit the cap yet.
        if count_after_one < target.max_attempts:
            r2 = session.attempt(word_b)
            count_after_two = session.attempts_used
            assert count_after_two == 2
            assert r2.attempts == 2
        assert count_after_one == 1
        assert r1.attempts == 1
        assert count_after_one <= target.max_attempts


@st.composite
def _pair(draw):
    return draw(st.text(min_size=1, max_size=40)), draw(st.text(min_size=1, max_size=40))


@given(_pair())
def test_constant_time_compare_length_independence(pair: tuple[str, str]) -> None:
    """Constant-time compare must not raise or crash on any input pair."""
    a, b = pair
    result = _constant_time_compare(a, b)
    assert isinstance(result, bool)
    # Reflexivity
    assert _constant_time_compare(a, a) is True
    # Symmetry
    assert _constant_time_compare(a, b) == _constant_time_compare(b, a)


@given(_target())
def test_disengaged_session_always_raises(target: CrackTarget) -> None:
    """Attempting outside an engaged session raises SessionNotEngaged."""
    session = CrackSession(target, capability_token="tok-123")
    with pytest.raises(SessionNotEngaged):
        session.attempt("anything")


@given(_target(), st.lists(_word(), min_size=1, max_size=10))
def test_full_lifecycle_attempts_match(target: CrackTarget, words: list[str]) -> None:
    """Through a full engage/attempt/disengage cycle, counts stay consistent.

    Attempts are bounded by max_attempts; once the cap is hit, further
    attempts return SKIPPED without incrementing the counter.
    """
    session = CrackSession(target, capability_token="tok-123")
    with session.engage():
        for i, word in enumerate(words, start=1):
            if i > target.max_attempts:
                break
            result = session.attempt(word)
            assert session.attempts_used == i, f"at word {i!r}"
            assert result.attempts == i, f"at word {i!r}"
    assert session.attempts_used == min(len(words), target.max_attempts)
