"""Configuration and data models for OSINT profiling."""

from __future__ import annotations
import json
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Any, Dict, List, Optional
from pathlib import Path


class ConfidenceTier(Enum):
    """Confidence classification tiers."""
    CONFIRMED = "confirmed"           # 0.90-1.00
    HIGH_CONFIDENCE = "high"          # 0.70-0.89
    MODERATE = "moderate"             # 0.40-0.69
    WEAK = "weak"                     # 0.10-0.39
    UNRESOLVED = "unresolved"         # 0.00-0.09

    @classmethod
    def from_score(cls, score: float) -> ConfidenceTier:
        if score >= 0.90:
            return cls.CONFIRMED
        elif score >= 0.70:
            return cls.HIGH_CONFIDENCE
        elif score >= 0.40:
            return cls.MODERATE
        elif score >= 0.10:
            return cls.WEAK
        return cls.UNRESOLVED


@dataclass(frozen=True, slots=True)
class Target:
    """Investigation target with seed identifiers."""
    alias: str
    aliases: List[str] = field(default_factory=list)
    telegram_usernames: List[str] = field(default_factory=list)
    facebook_ids: List[str] = field(default_factory=list)
    instagram_usernames: List[str] = field(default_factory=list)
    emails: List[str] = field(default_factory=list)
    domains: List[str] = field(default_factory=list)
    real_name_candidates: List[str] = field(default_factory=list)
    location_indicators: List[str] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)

    def all_aliases(self) -> List[str]:
        """All known aliases including primary."""
        result = [self.alias]
        result.extend(self.aliases)
        return list(dict.fromkeys(result))  # dedupe preserving order


@dataclass(frozen=True, slots=True)
class Evidence:
    """Single piece of evidence from a source."""
    source: str
    target_alias: str
    data_type: str
    raw_value: str
    confidence: float
    weight: float
    timestamp: str = field(default_factory=lambda: datetime.utcnow().isoformat())
    metadata: Dict[str, Any] = field(default_factory=dict)

    def weighted_score(self) -> float:
        return self.confidence * self.weight


@dataclass(frozen=True, slots=True)
class Profile:
    """Aggregated profile for a target."""
    target: Target
    evidence: List[Evidence] = field(default_factory=list)
    aggregate_confidence: float = 0.0
    tier: ConfidenceTier = ConfidenceTier.UNRESOLVED
    real_name_candidates: List[str] = field(default_factory=list)
    location_profile: Dict[str, Any] = field(default_factory=dict)
    generated_at: str = field(default_factory=lambda: datetime.utcnow().isoformat())

    def to_dict(self) -> Dict[str, Any]:
        return {
            "target": {
                "alias": self.target.alias,
                "aliases": self.target.all_aliases(),
                "telegram_usernames": self.target.telegram_usernames,
                "facebook_ids": self.target.facebook_ids,
                "instagram_usernames": self.target.instagram_usernames,
                "emails": self.target.emails,
                "domains": self.target.domains,
                "real_name_candidates": self.target.real_name_candidates,
                "location_indicators": self.target.location_indicators,
            },
            "aggregate_confidence": self.aggregate_confidence,
            "tier": self.tier.value,
            "real_name_candidates": self.real_name_candidates,
            "location_profile": self.location_profile,
            "evidence_count": len(self.evidence),
            "generated_at": self.generated_at,
            "evidence": [
                {
                    "source": e.source,
                    "target_alias": e.target_alias,
                    "data_type": e.data_type,
                    "raw_value": e.raw_value,
                    "confidence": e.confidence,
                    "weight": e.weight,
                    "weighted_score": e.weighted_score(),
                    "timestamp": e.timestamp,
                    "metadata": e.metadata,
                }
                for e in self.evidence
            ],
        }


@dataclass(frozen=True, slots=True)
class DetectionResult:
    """Result of auto-detection from a source."""
    target: Target
    source: str
    confidence: float
    raw_data: Dict[str, Any]


def load_targets_from_json(path: str | Path) -> Dict[str, Target]:
    """Load targets from JSON seed registry."""
    with open(path, 'r') as f:
        data = json.load(f)
    
    targets = {}
    for alias, info in data.items():
        targets[alias] = Target(
            alias=alias,
            aliases=info.get("aliases", []),
            telegram_usernames=info.get("telegram_usernames", []),
            facebook_ids=info.get("facebook_ids", []),
            instagram_usernames=info.get("instagram_usernames", []),
            emails=info.get("emails", []),
            domains=info.get("domains", []),
            real_name_candidates=info.get("real_name_candidates", []),
            location_indicators=info.get("location_indicators", []),
            metadata=info.get("metadata", {}),
        )
    return targets


def save_targets_to_json(targets: Dict[str, Target], path: str | Path) -> None:
    """Save targets to JSON seed registry."""
    data = {}
    for alias, target in targets.items():
        data[alias] = {
            "aliases": target.aliases,
            "telegram_usernames": target.telegram_usernames,
            "facebook_ids": target.facebook_ids,
            "instagram_usernames": target.instagram_usernames,
            "emails": target.emails,
            "domains": target.domains,
            "real_name_candidates": target.real_name_candidates,
            "location_indicators": target.location_indicators,
            "metadata": target.metadata,
        }
    with open(path, 'w') as f:
        json.dump(data, f, indent=2)