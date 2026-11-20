"""Unit tests for osint_profiler core models and correlator."""

from __future__ import annotations

import sys
from pathlib import Path

# Ensure osint_profiler package is importable when running tests directly
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from osint_profiler.core.models import (  # noqa: E402
    EvidencePoint,
    LocationIndicator,
    PlatformPresence,
    TargetProfile,
)
from osint_profiler.correlator import (  # noqa: E402
    classify_confidence,
    compute_aggregate_confidence,
    correlate,
    extract_location,
    extract_real_name_candidates,
)
from osint_profiler.config.settings import get_settings  # noqa: E402


def test_evidence_point_weighted_score():
    point = EvidencePoint(
        source="test",
        data_type="real_name",
        value="John Doe",
        weight=0.9,
        confidence=0.8,
    )
    assert point.weighted_score() == 0.72


def test_target_profile_immutable():
    profile = TargetProfile(alias="test_alias")
    original_evidence_count = len(profile.evidence)
    new_profile = profile.add_evidence(
        EvidencePoint(
            source="test",
            data_type="real_name",
            value="Jane Doe",
            weight=0.9,
            confidence=0.8,
        )
    )
    # Original unchanged
    assert len(profile.evidence) == original_evidence_count
    # New profile has evidence
    assert len(new_profile.evidence) == original_evidence_count + 1
    assert new_profile.alias == "test_alias"


def test_compute_aggregate_confidence_empty():
    assert compute_aggregate_confidence([]) == 0.0


def test_compute_aggregate_confidence_single():
    evidence = [
        EvidencePoint(
            source="test",
            data_type="real_name",
            value="John",
            weight=0.9,
            confidence=0.8,
        )
    ]
    # 1 - (1 - 0.72) = 0.72
    assert compute_aggregate_confidence(evidence) == 0.72


def test_classify_confidence():
    tiers = get_settings().tiers
    assert classify_confidence(0.95, tiers) == "confirmed"
    assert classify_confidence(0.80, tiers) == "high_candidate"
    assert classify_confidence(0.50, tiers) == "moderate_candidate"
    assert classify_confidence(0.20, tiers) == "weak_lead"
    assert classify_confidence(0.00, tiers) == "unresolved"


def test_extract_real_name_candidates():
    evidence = [
        EvidencePoint(source="fb", data_type="real_name", value="Ahmed Salar", weight=0.9, confidence=0.85),
        EvidencePoint(source="ig", data_type="real_name", value="Ahmed Salar", weight=0.75, confidence=0.70),
        EvidencePoint(source="fb", data_type="real_name", value="ahmd osam", weight=0.75, confidence=0.60),
        EvidencePoint(source="fb", data_type="location", value="PH", weight=0.6, confidence=0.5),
    ]
    candidates = extract_real_name_candidates(evidence)
    assert "Ahmed Salar" in candidates
    assert "ahmd osam" in candidates
    assert len(candidates) == 2  # deduplicated


def test_extract_location():
    evidence = [
        EvidencePoint(
            source="whois",
            data_type="location",
            value="Philippines",
            weight=0.6,
            confidence=0.70,
            metadata={"country": "PH", "region": "NCR", "city": "Manila"},
        )
    ]
    loc = extract_location(evidence)
    assert loc["country"] == "PH"
    assert loc["region"] == "NCR"
    assert loc["city"] == "Manila"
    assert loc["confidence"] == 0.70


def test_correlate_full_pipeline():
    settings = get_settings()
    profile = TargetProfile(alias="test_user")
    profile = profile.add_evidence(
        EvidencePoint(
            source="facebook",
            data_type="real_name",
            value="John Smith",
            weight=settings.weights.direct_real_name,
            confidence=0.85,
        )
    )
    profile = profile.add_evidence(
        EvidencePoint(
            source="instagram",
            data_type="real_name",
            value="John Smith",
            weight=settings.weights.corroborating_name,
            confidence=0.75,
        )
    )
    result = correlate(profile, settings)
    assert result.aggregate_confidence > 0.0
    assert result.classification in ("confirmed", "high_candidate", "moderate_candidate", "weak_lead", "unresolved")
    assert "John Smith" in result.real_name_candidates


def test_platform_presence_creation():
    presence = PlatformPresence(
        platform="facebook",
        handle="test_handle",
        user_id="12345",
        url="https://facebook.com/test_handle",
        accessible=True,
        data_extracted={"page_name": "Test Page"},
        notes="Test note",
    )
    assert presence.platform == "facebook"
    assert presence.handle == "test_handle"
    assert presence.accessible is True


def test_location_indicator_defaults():
    loc = LocationIndicator()
    assert loc.country is None
    assert loc.region is None
    assert loc.city is None
    assert loc.basis == ""
    assert loc.confidence == 0.0


__all__ = [
    "test_evidence_point_weighted_score",
    "test_target_profile_immutable",
    "test_compute_aggregate_confidence_empty",
    "test_compute_aggregate_confidence_single",
    "test_classify_confidence",
    "test_extract_real_name_candidates",
    "test_extract_location",
    "test_correlate_full_pipeline",
    "test_platform_presence_creation",
    "test_location_indicator_defaults",
]
