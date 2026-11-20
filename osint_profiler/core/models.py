"""Core data models for osint_profiler.

All models are frozen slotted dataclasses per DEVELOPMENT_STANDARDS.md.
Immutable evidence records ensure reproducibility and auditability.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any


@dataclass(frozen=True, slots=True)
class EvidencePoint:
    """A single piece of evidence collected from a source."""

    source: str
    data_type: str  # e.g., "real_name", "location", "email", "platform_id"
    value: str
    weight: float  # 0.0-1.0 importance weight
    confidence: float  # 0.0-1.0 source reliability
    raw_snippet: str = ""
    url: str | None = None
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    metadata: dict[str, Any] = field(default_factory=dict)

    def weighted_score(self) -> float:
        """Return weight * confidence for this evidence point."""
        return round(self.weight * self.confidence, 4)


@dataclass(frozen=True, slots=True)
class PlatformPresence:
    """Record of a target's presence on a platform."""

    platform: str  # facebook, instagram, telegram, etc.
    handle: str | None = None
    user_id: str | None = None
    url: str | None = None
    accessible: bool = False
    data_extracted: dict[str, Any] = field(default_factory=dict)
    notes: str = ""


@dataclass(frozen=True, slots=True)
class LocationIndicator:
    """A geographic indicator extracted from evidence."""

    country: str | None = None
    region: str | None = None
    city: str | None = None
    basis: str = ""  # e.g., ".ph domain", "Filipino language", "hashtag #PH"
    confidence: float = 0.0


@dataclass(frozen=True, slots=True)
class TargetProfile:
    """Complete profiling record for a single target alias."""

    alias: str
    real_name_candidates: list[str] = field(default_factory=list)
    real_name_confidence: float = 0.0
    location: LocationIndicator = field(default_factory=LocationIndicator)
    platforms: list[PlatformPresence] = field(default_factory=list)
    evidence: list[EvidencePoint] = field(default_factory=list)
    infrastructure: dict[str, str] = field(default_factory=dict)  # domain -> registrar/IP
    aggregate_confidence: float = 0.0
    classification: str = "unresolved"  # confirmed, high_candidate, moderate, weak, unresolved
    last_updated: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )

    def add_evidence(self, point: EvidencePoint) -> TargetProfile:
        """Return a new TargetProfile with evidence appended (immutable)."""
        return TargetProfile(
            alias=self.alias,
            real_name_candidates=self.real_name_candidates,
            real_name_confidence=self.real_name_confidence,
            location=self.location,
            platforms=self.platforms,
            evidence=list(self.evidence) + [point],
            infrastructure=self.infrastructure,
            aggregate_confidence=self.aggregate_confidence,
            classification=self.classification,
            last_updated=self.last_updated,
        )

    def add_platform(self, presence: PlatformPresence) -> TargetProfile:
        """Return a new TargetProfile with platform presence appended."""
        return TargetProfile(
            alias=self.alias,
            real_name_candidates=self.real_name_candidates,
            real_name_confidence=self.real_name_confidence,
            location=self.location,
            platforms=list(self.platforms) + [presence],
            evidence=self.evidence,
            infrastructure=self.infrastructure,
            aggregate_confidence=self.aggregate_confidence,
            classification=self.classification,
            last_updated=self.last_updated,
        )

    def to_dict(self) -> dict[str, Any]:
        """Serialize to dictionary for JSON output."""
        return {
            "alias": self.alias,
            "real_name_candidates": self.real_name_candidates,
            "real_name_confidence": self.real_name_confidence,
            "location": {
                "country": self.location.country,
                "region": self.location.region,
                "city": self.location.city,
                "basis": self.location.basis,
                "confidence": self.location.confidence,
            },
            "platforms": [
                {
                    "platform": p.platform,
                    "handle": p.handle,
                    "user_id": p.user_id,
                    "url": p.url,
                    "accessible": p.accessible,
                    "data_extracted": p.data_extracted,
                    "notes": p.notes,
                }
                for p in self.platforms
            ],
            "evidence": [
                {
                    "source": e.source,
                    "data_type": e.data_type,
                    "value": e.value,
                    "weight": e.weight,
                    "confidence": e.confidence,
                    "weighted_score": e.weighted_score(),
                    "raw_snippet": e.raw_snippet,
                    "url": e.url,
                    "timestamp": e.timestamp,
                    "metadata": e.metadata,
                }
                for e in self.evidence
            ],
            "infrastructure": self.infrastructure,
            "aggregate_confidence": self.aggregate_confidence,
            "classification": self.classification,
            "last_updated": self.last_updated,
        }


@dataclass(frozen=True, slots=True)
class InvestigationReport:
    """Final report container for one or more targets."""

    targets: dict[str, TargetProfile]
    methodology: str = "osint_profiler v1.0"
    generated_at: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    total_targets: int = field(init=False)
    confirmed_count: int = field(init=False)
    unresolved_count: int = field(init=False)

    def __post_init__(self) -> None:
        object.__setattr__(self, "total_targets", len(self.targets))
        object.__setattr__(
            self,
            "confirmed_count",
            sum(1 for t in self.targets.values() if t.classification == "confirmed"),
        )
        object.__setattr__(
            self,
            "unresolved_count",
            sum(1 for t in self.targets.values() if t.classification == "unresolved"),
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "methodology": self.methodology,
            "generated_at": self.generated_at,
            "summary": {
                "total_targets": self.total_targets,
                "confirmed": self.confirmed_count,
                "unresolved": self.unresolved_count,
            },
            "targets": {alias: profile.to_dict() for alias, profile in self.targets.items()},
        }


__all__ = [
    "EvidencePoint",
    "PlatformPresence",
    "LocationIndicator",
    "TargetProfile",
    "InvestigationReport",
]
