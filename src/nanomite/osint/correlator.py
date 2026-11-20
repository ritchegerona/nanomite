"""Cross-source identity correlation and confidence scoring.

Combines evidence from identity mapping, social enumeration, email
harvesting, and metadata extraction into a unified identity profile
with a computed confidence score.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from .identity_mapper import IdentityMapper, IdentityCandidate, EvidenceRecord


@dataclass
class IdentityProfile:
    """Final correlated identity profile."""
    primary_alias: str
    real_name: Optional[str]
    confidence: float  # 0.0 - 1.0
    evidence_count: int
    aliases: list[str] = field(default_factory=list)
    affiliations: list[str] = field(default_factory=list)
    possible_employers: list[str] = field(default_factory=list)
    social_profiles: list[dict] = field(default_factory=list)
    emails: list[dict] = field(default_factory=list)
    metadata_findings: list[dict] = field(default_factory=list)
    evidence: list[dict] = field(default_factory=list)

    def to_dict(self) -> dict:
        return {
            "primary_alias": self.primary_alias,
            "real_name": self.real_name,
            "confidence": self.confidence,
            "evidence_count": self.evidence_count,
            "aliases": self.aliases,
            "affiliations": self.affiliations,
            "possible_employers": self.possible_employers,
            "social_profiles": self.social_profiles,
            "emails": self.emails,
            "metadata_findings": self.metadata_findings,
            "evidence": self.evidence,
        }


class Correlator:
    """Correlate multiple OSINT sources into a single identity profile."""

    def __init__(self, mapper: IdentityMapper):
        self.mapper = mapper

    def build_profile(
        self,
        alias: str,
        social_profiles: list[dict] | None = None,
        emails: list[dict] | None = None,
        metadata_findings: list[dict] | None = None,
    ) -> IdentityProfile:
        cand = self.mapper.get_candidate(alias)
        if not cand:
            raise ValueError(f"No identity candidate for alias: {alias}")

        aliases = [alias] + cand.aliases_seen

        profile = IdentityProfile(
            primary_alias=alias,
            real_name=cand.real_name,
            confidence=cand.overall_confidence,
            evidence_count=len(cand.evidence),
            aliases=aliases,
            affiliations=list(dict.fromkeys(cand.affiliations)),
            possible_employers=list(dict.fromkeys(cand.possible_employers)),
            social_profiles=social_profiles or [],
            emails=emails or [],
            metadata_findings=metadata_findings or [],
            evidence=[ev.to_dict() for ev in cand.evidence],
        )
        return profile

    def score_confidence(self, profile: IdentityProfile) -> float:
        """Re-score confidence incorporating social/email/metadata corroboration."""
        base = profile.confidence
        bonuses = 0.0
        if profile.social_profiles:
            bonuses += 0.1 * min(len(profile.social_profiles), 3)
        if profile.emails:
            bonuses += 0.1 * min(len(profile.emails), 2)
        if profile.metadata_findings:
            bonuses += 0.1 * min(len(profile.metadata_findings), 2)
        return min(1.0, base + bonuses)