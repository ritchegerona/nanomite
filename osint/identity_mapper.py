"""Identity mapper: alias-to-real-name correlation engine.

Provides structured correlation of hacker aliases / handles to real-world
identities using publicly available OSINT sources. Results are scored by
source confidence and stored as evidence records.

Authorized security research only.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field, asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional


@dataclass
class EvidenceRecord:
    """A single piece of evidence linking an alias to an identity."""
    source: str
    source_url: Optional[str]
    date_observed: Optional[str]
    quote: str
    confidence: float  # 0.0 - 1.0
    tags: list[str] = field(default_factory=list)

    def to_dict(self) -> dict:
        return asdict(self)


@dataclass
class IdentityCandidate:
    """A candidate real identity for an alias."""
    alias: str
    real_name: Optional[str]
    evidence: list[EvidenceRecord] = field(default_factory=list)
    overall_confidence: float = 0.0
    aliases_seen: list[str] = field(default_factory=list)
    affiliations: list[str] = field(default_factory=list)
    possible_employers: list[str] = field(default_factory=list)
    social_profiles: list[dict] = field(default_factory=list)

    def add_evidence(self, record: EvidenceRecord) -> None:
        self.evidence.append(record)
        self._recompute_confidence()

    def _recompute_confidence(self) -> None:
        if not self.evidence:
            self.overall_confidence = 0.0
            return
        # Weighted average: direct alias-to-name statements score highest
        weights = {
            "direct_alias_name": 1.0,
            "corroborating_news": 0.8,
            "social_profile_match": 0.6,
            "affiliation_mention": 0.4,
        }
        total = 0.0
        wsum = 0.0
        for ev in self.evidence:
            w = 0.5  # default
            for tag in ev.tags:
                if tag in weights:
                    w = weights[tag]
                    break
            total += ev.confidence * w
            wsum += w
        self.overall_confidence = total / wsum if wsum else 0.0


class IdentityMapper:
    """Correlates aliases to real identities using OSINT evidence."""

    def __init__(self, workspace: str = "."):
        self.workspace = Path(workspace)
        self.identities: dict[str, IdentityCandidate] = {}

    def create_candidate(self, alias: str, real_name: Optional[str] = None) -> IdentityCandidate:
        cand = IdentityCandidate(alias=alias, real_name=real_name)
        self.identities[alias] = cand
        return cand

    def get_candidate(self, alias: str) -> Optional[IdentityCandidate]:
        return self.identities.get(alias)

    def add_evidence(
        self,
        alias: str,
        source: str,
        quote: str,
        confidence: float,
        tags: list[str],
        source_url: Optional[str] = None,
        date_observed: Optional[str] = None,
        real_name: Optional[str] = None,
    ) -> IdentityCandidate:
        cand = self.get_candidate(alias)
        if cand is None:
            cand = self.create_candidate(alias, real_name)
        elif real_name and not cand.real_name:
            cand.real_name = real_name
        record = EvidenceRecord(
            source=source,
            source_url=source_url,
            date_observed=date_observed or datetime.now(timezone.utc).isoformat(),
            quote=quote,
            confidence=confidence,
            tags=tags,
        )
        cand.add_evidence(record)
        return cand

    def merge_alias(self, primary_alias: str, secondary_alias: str) -> None:
        """Treat two aliases as the same person, merging evidence."""
        primary = self.get_candidate(primary_alias)
        secondary = self.get_candidate(secondary_alias)
        if not primary or not secondary:
            return
        primary.evidence.extend(secondary.evidence)
        primary.social_profiles.extend(secondary.social_profiles)
        primary.affiliations.extend(secondary.affiliations)
        primary.possible_employers.extend(secondary.possible_employers)
        if secondary.alias not in primary.aliases_seen:
            primary.aliases_seen.append(secondary.alias)
        primary._recompute_confidence()
        del self.identities[secondary.alias]

    def export_json(self, path: str | Path) -> None:
        data = {alias: asdict(cand) for alias, cand in self.identities.items()}
        Path(path).write_text(json.dumps(data, indent=2), encoding="utf-8")

    def summary(self) -> str:
        lines = ["IDENTITY MAPPING SUMMARY", "=" * 40]
        for alias, cand in self.identities.items():
            lines.append(f"\nAlias: {alias}")
            lines.append(f"  Real name: {cand.real_name or 'UNKNOWN'}")
            lines.append(f"  Confidence: {cand.overall_confidence:.2f}")
            lines.append(f"  Evidence items: {len(cand.evidence)}")
            lines.append(f"  Affiliations: {', '.join(cand.affiliations) or 'none'}")
            lines.append(f"  Possible employers: {', '.join(cand.possible_employers) or 'none'}")
            for ev in cand.evidence:
                lines.append(f"    - [{ev.source}] {ev.quote[:100]}... (conf={ev.confidence})")
        return "\n".join(lines)