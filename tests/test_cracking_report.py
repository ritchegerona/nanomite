"""Tests for the crack report formatting module."""

from __future__ import annotations

import pytest

from nanomite.cracking.engine import CrackOutcome, CrackResult
from nanomite.cracking.report import CrackReport, to_json, to_text


def _result(success: bool, plaintext: str | None = None) -> CrackResult:
    return CrackResult(
        target_path="/tmp/t.hash",
        scope_id="eng-001",
        outcome=CrackOutcome.SUCCESS if success else CrackOutcome.FAILURE,
        plaintext=plaintext,
        attempts=1 if success else 5,
        elapsed_ms=10 if success else 50,
        timestamp_utc="2024-01-01T00:00:00+00:00",
    )


def test_report_properties() -> None:
    results = (_result(True, "pw1"), _result(False), _result(True, "pw2"))
    report = CrackReport(scope_id="eng-001", results=results)
    assert report.success_count == 2
    assert report.total_attempts == 7
    assert len(report.results) == 3


def test_text_report_contains_cracked_entries() -> None:
    results = (_result(True, "secret"), _result(False))
    report = CrackReport(scope_id="eng-001", results=results)
    text = to_text(report)
    assert "CRACKED" in text
    assert "secret" in text
    assert "eng-001" in text


def test_text_report_no_successes() -> None:
    report = CrackReport(scope_id="eng-001", results=(_result(False),))
    text = to_text(report)
    assert "no plaintext recovered" in text


def test_json_report_structure() -> None:
    results = (_result(True, "pw1"),)
    report = CrackReport(scope_id="eng-001", results=results)
    js = to_json(report)
    assert '"scope_id": "eng-001"' in js
    assert '"plaintext": "pw1"' in js
    assert '"success_count": 1' in js


def test_results_are_immutable() -> None:
    report = CrackReport(scope_id="s", results=(_result(False),))
    with pytest.raises(AttributeError):
        report.scope_id = "x"  # type: ignore[misc]
