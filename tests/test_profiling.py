"""Tests for OSINT Profiling Framework."""

import pytest
from osint.profiling.config import Target, Evidence, ConfidenceTier, load_targets_from_json
from osint.profiling.engine import _correlate_and_score, _extract_real_names, _build_location_profile
from osint.profiling.detector import TargetDetector


class TestConfig:
    """Test configuration models."""
    
    def test_target_creation(self):
        target = Target(
            alias="testtarget",
            aliases=["test_target"],
            telegram_usernames=["@testtarget"],
            facebook_ids=["123456789"],
            instagram_usernames=["testtarget"],
            emails=["test@example.com"],
            domains=["test.ph"],
            real_name_candidates=["Test User"],
            location_indicators=["Philippines"],
        )
        assert target.alias == "testtarget"
        assert "test_target" in target.all_aliases()
        assert len(target.telegram_usernames) == 1
    
    def test_evidence_weighted_score(self):
        ev = Evidence(
            source="facebook",
            target_alias="test",
            data_type="og:title",
            raw_value="Test User",
            confidence=0.9,
            weight=0.95,
        )
        assert ev.weighted_score() == pytest.approx(0.855)
    
    def test_confidence_tiers(self):
        assert ConfidenceTier.from_score(0.95) == ConfidenceTier.CONFIRMED
        assert ConfidenceTier.from_score(0.80) == ConfidenceTier.HIGH_CONFIDENCE
        assert ConfidenceTier.from_score(0.55) == ConfidenceTier.MODERATE
        assert ConfidenceTier.from_score(0.25) == ConfidenceTier.WEAK
        assert ConfidenceTier.from_score(0.05) == ConfidenceTier.UNRESOLVED


class TestEngine:
    """Test profiling engine."""
    
    def test_correlate_and_score_no_evidence(self):
        target = Target(alias="empty")
        profile = _correlate_and_score(target, [])
        assert profile.aggregate_confidence == 0.0
        assert profile.tier == ConfidenceTier.UNRESOLVED
    
    def test_extract_real_names(self):
        target = Target(alias="test", real_name_candidates=["Seed Name"])
        evidence = [
            Evidence(
                source="facebook",
                target_alias="test",
                data_type="og:title",
                raw_value="John Doe (@johndoe)",
                confidence=0.9,
                weight=0.95,
            ),
            Evidence(
                source="instagram",
                target_alias="test",
                data_type="profile:full_name",
                raw_value="John Doe",
                confidence=0.85,
                weight=0.9,
            ),
        ]
        names = _extract_real_names(evidence, target)
        assert "Seed Name" in names
        assert "John Doe" in names
    
    def test_build_location_profile_philippines(self):
        target = Target(alias="test", location_indicators=["Philippines"])
        evidence = [
            Evidence(
                source="facebook",
                target_alias="test",
                data_type="og:locale",
                raw_value="fil_PH",
                confidence=0.85,
                weight=0.9,
            ),
        ]
        loc = _build_location_profile(evidence, target)
        assert loc.get("likely_country") == "Philippines"
        assert loc.get("language") == "fil"
        assert loc.get("confidence", 0) > 0


class TestDetector:
    """Test target detector."""
    
    def test_normalize_alias(self):
        detector = TargetDetector()
        assert detector._normalize_alias("@testuser", "telegram") == "testuser"
        assert detector._normalize_alias("user@domain.com", "email") == "user"
        # facebook_id: raw digits are filtered by skip_patterns (r'^\d+$')
        # Only works when extracted via regex with facebook.com/ prefix
        assert detector._normalize_alias("test.ph", "domain_ph") == "test"
        assert detector._normalize_alias("http://example.com", "telegram") is None
        assert detector._normalize_alias("ab", "telegram") is None  # too short


class TestIntegration:
    """Integration tests using actual targets.json."""
    
    def test_load_targets_json(self):
        targets = load_targets_from_json("osint/profiling/targets.json")
        assert len(targets) >= 6
        assert "nullsecphilippines" in targets
        assert "happygoluckyph" in targets
        assert "crimsonsecphilippines" in targets
        assert targets["crimsonsecphilippines"].real_name_candidates == ["Mark Jhurneil Morales"]


if __name__ == "__main__":
    pytest.main([__file__, "-v"])