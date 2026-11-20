"""Evidence correlation and scoring module.

Handles cross-source evidence aggregation, confidence calculation,
and classification logic. Pure computation with no I/O.
"""

from __future__ import annotations

import logging
from typing import Any

from ..config.settings import Settings, ScoringWeights, ConfidenceTiers
from ..core.models import EvidencePoint, TargetProfile

logger = logging.getLogger(__name__)


def compute_aggregate_confidence(evidence: list[EvidencePoint]) -> float:
    """Compute aggregate confidence using the independence formula.

    aggregate = 1 - Π(1 - weight_i × confidence_i)

    Source status entries (weight=0) are excluded from the calculation.
    """
    relevant = [e for e in evidence if e.data_type != "source_status"]
    if not relevant:
        return 0.0

    aggregate = 1.0
    for point in relevant:
        aggregate *= 1.0 - point.weighted_score()
    return round(1.0 - aggregate, 4)


def classify_confidence(score: float, tiers: ConfidenceTiers) -> str:
    """Classify a confidence score into a tier label."""
    if score >= tiers.confirmed[0]:
        return "confirmed"
    if score >= tiers.high_candidate[0]:
        return "high_candidate"
    if score >= tiers.moderate_candidate[0]:
        return "moderate_candidate"
    if score >= tiers.weak_lead[0]:
        return "weak_lead"
    return "unresolved"


def extract_real_name_candidates(evidence: list[EvidencePoint]) -> list[str]:
    """Extract real name candidates from evidence with confidence > 0.5."""
    seen: set[str] = set()
    candidates: list[str] = []
    for point in evidence:
        if point.data_type == "real_name" and point.confidence > 0.5:
            if point.value not in seen:
                seen.add(point.value)
                candidates.append(point.value)
    return candidates


def extract_location(evidence: list[EvidencePoint]) -> dict[str, Any]:
    """Extract the best location indicator from evidence."""
    location_evidence = [e for e in evidence if e.data_type == "location"]
    if not location_evidence:
        return {"country": None, "region": None, "city": None, "basis": "", "confidence": 0.0}

    best = max(location_evidence, key=lambda e: e.confidence)
    return {
        "country": best.metadata.get("country"),
        "region": best.metadata.get("region"),
        "city": best.metadata.get("city"),
        "basis": best.value,
        "confidence": best.confidence,
    }


def correlate(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Run correlation and scoring on a TargetProfile.

    Returns a new TargetProfile with updated aggregate confidence,
    classification, real name candidates, and location.
    """
    logger.info("Correlating evidence for %s", profile.alias)

    aggregate = compute_aggregate_confidence(profile.evidence)
    classification = classify_confidence(aggregate, settings.tiers)
    name_candidates = extract_real_name_candidates(profile.evidence)
    location_data = extract_location(profile.evidence)

    from ..core.models import LocationIndicator

    location = LocationIndicator(
        country=location_data["country"],
        region=location_data["region"],
        city=location_data["city"],
        basis=location_data["basis"],
        confidence=location_data["confidence"],
    )

    real_name_confidence = aggregate if name_candidates else 0.0

    return TargetProfile(
        alias=profile.alias,
        real_name_candidates=name_candidates,
        real_name_confidence=real_name_confidence,
        location=location,
        platforms=profile.platforms,
        evidence=profile.evidence,
        infrastructure=profile.infrastructure,
        aggregate_confidence=aggregate,
        classification=classification,
        last_updated=profile.last_updated,
    )


def correlate_all(
    profiles: dict[str, TargetProfile], settings: Settings
) -> dict[str, TargetProfile]:
    """Correlate all profiles in a dictionary."""
    return {alias: correlate(profile, settings) for alias, profile in profiles.items()}


__all__ = [
    "compute_aggregate_confidence",
    "classify_confidence",
    "extract_real_name_candidates",
    "extract_location",
    "correlate",
    "correlate_all",
]
