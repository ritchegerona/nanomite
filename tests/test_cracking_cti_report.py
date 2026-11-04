"""Tests for the CTI report generator."""

from __future__ import annotations

import hashlib
import json

import pytest

from nanomite.cracking.cti_report import (
    CrackEvidence,
    CtiReport,
    Engagement,
    evidence_from_result,
    to_json,
    to_markdown,
    to_sarif,
    write_report,
)
from nanomite.cracking.engine import CrackOutcome, CrackResult


def _engagement() -> Engagement:
    return Engagement(
        engagement_id="eng-cti-001",
        scope_id="eng-001",
        operator="analyst-1",
        authorized_at="2024-01-01T00:00:00+00:00",
        targets=("10.0.0.9", "example.com"),
    )


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


def test_engagement_and_evidence_are_immutable() -> None:
    eng = _engagement()
    with pytest.raises(AttributeError):
        eng.operator = "x"  # type: ignore[misc]

    ev = CrackEvidence(
        target_path="/tmp/t.hash",
        scope_id="eng-001",
        hash_type="sha256",
        hash_value=hashlib.sha256(b"pw").hexdigest(),
        plaintext="pw",
        attempts=3,
        elapsed_ms=10,
        timestamp_utc="2024-01-01T00:00:00+00:00",
        engagement_id="eng-cti-001",
    )
    with pytest.raises(AttributeError):
        ev.plaintext = "changed"  # type: ignore[misc]


def test_evidence_from_result_requires_success() -> None:
    eng = _engagement()
    failed = _result(False)
    with pytest.raises(ValueError):
        evidence_from_result(
            result=failed,
            engagement=eng,
            target_path="/tmp/t.hash",
            hash_type="sha256",
            hash_value="abc",
        )


def test_evidence_from_result_builds() -> None:
    eng = _engagement()
    digest = hashlib.sha256(b"password").hexdigest()
    success = _result(True, "password")
    ev = evidence_from_result(
        result=success,
        engagement=eng,
        target_path="/tmp/t.hash",
        hash_type="sha256",
        hash_value=digest,
    )
    assert ev.plaintext == "password"
    assert ev.engagement_id == "eng-cti-001"
    assert ev.attempts == 1


def test_report_properties() -> None:
    eng = _engagement()
    digest = hashlib.sha256(b"pw").hexdigest()
    ev = CrackEvidence(
        target_path="/tmp/t.hash",
        scope_id="eng-001",
        hash_type="sha256",
        hash_value=digest,
        plaintext="pw",
        attempts=3,
        elapsed_ms=10,
        timestamp_utc="2024-01-01T00:00:00+00:00",
        engagement_id="eng-cti-001",
    )
    report = CtiReport(engagement=eng, evidence=(ev,))
    assert report.cracked_count == 1
    assert report.total_attempts == 3
    assert list(report.scope_ids) == ["eng-001"]


def test_to_json_structure() -> None:
    eng = _engagement()
    report = CtiReport(engagement=eng, evidence=())
    js = json.loads(to_json(report))
    assert js["report_type"] == "crack_audit"
    assert js["engagement"]["engagement_id"] == "eng-cti-001"
    assert js["summary"]["cracked_count"] == 0


def test_to_sarif_structure() -> None:
    eng = _engagement()
    report = CtiReport(engagement=eng, evidence=())
    sarif = json.loads(to_sarif(report))
    assert sarif["version"] == "2.1.0"
    assert sarif["runs"][0]["tool"]["driver"]["name"] == "nanomite-crack"


def test_to_markdown_contains_engagement() -> None:
    eng = _engagement()
    report = CtiReport(engagement=eng, evidence=())
    md = to_markdown(report)
    assert "eng-cti-001" in md
    assert "analyst-1" in md


def test_write_report_creates_file(tmp_path) -> None:
    eng = _engagement()
    report = CtiReport(engagement=eng, evidence=())
    p = tmp_path / "report.json"
    write_report(report, str(p), fmt="json")
    assert p.is_file()
    data = json.loads(p.read_text(encoding="utf-8"))
    assert data["engagement"]["engagement_id"] == "eng-cti-001"
