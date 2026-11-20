"""Main profiling engine.

Orchestrates source modules, correlates evidence, and produces
TargetProfile objects. All network access is plan-only unless
explicitly enabled via the `allow_active` flag.
"""

from __future__ import annotations

import logging
from typing import Any

from ..config.settings import get_settings
from ..core.exceptions import (
    AuthenticationError,
    OsintProfilerError,
    RateLimitedError,
    SourceUnavailableError,
)
from ..core.models import (
    EvidencePoint,
    InvestigationReport,
    LocationIndicator,
    PlatformPresence,
    TargetProfile,
)

logger = logging.getLogger(__name__)


class ProfilingEngine:
    """Core engine that runs the profiling pipeline against one or more aliases."""

    def __init__(self, allow_active: bool = False) -> None:
        self.settings = get_settings()
        self.allow_active = allow_active
        self._sources: dict[str, Any] = {}
        logger.info("ProfilingEngine initialized (allow_active=%s)", allow_active)

    def register_source(self, name: str, source_module: Any) -> None:
        """Register a data source module.

        source_module should be the module object (not just the gather function)
        so that ACTIVE_ONLY can be read from module-level attributes.
        """
        self._sources[name] = source_module
        logger.debug("Registered source: %s", name)

    def profile_alias(self, alias: str) -> TargetProfile:
        """Run the full profiling pipeline for a single alias.

        Returns an immutable TargetProfile with all collected evidence.
        """
        logger.info("Starting profile for alias: %s", alias)
        profile = TargetProfile(alias=alias)

        # Phase 1: Seed enumeration (no network required)
        profile = self._enumerate_seeds(profile)

        # Phase 2: Passive OSINT gathering
        profile = self._run_passive_sources(profile)

        # Phase 3: Active reconnaissance (only if authorized)
        if self.allow_active:
            profile = self._run_active_sources(profile)
        else:
            logger.info("Active reconnaissance disabled for alias: %s", alias)

        # Phase 4: Correlation and scoring
        profile = self._correlate_and_score(profile)

        logger.info(
            "Profile complete for %s: confidence=%.2f, classification=%s",
            alias,
            profile.aggregate_confidence,
            profile.classification,
        )
        return profile

    def profile_multiple(self, aliases: list[str]) -> InvestigationReport:
        """Profile multiple aliases and return an InvestigationReport."""
        targets = {alias: self.profile_alias(alias) for alias in aliases}
        return InvestigationReport(targets=targets)

    # ------------------------------------------------------------------
    # Internal pipeline stages
    # ------------------------------------------------------------------

    def _enumerate_seeds(self, profile: TargetProfile) -> TargetProfile:
        """Generate seed variations without network access."""
        logger.debug("Enumerating seeds for %s", profile.alias)
        # Seed enumeration is pure computation; no I/O needed here.
        # Source modules will consume the alias directly.
        return profile

    def _run_passive_sources(self, profile: TargetProfile) -> TargetProfile:
        """Execute all registered passive sources."""
        for name, source_fn in self._sources.items():
            if getattr(source_fn, "ACTIVE_ONLY", False):
                continue
            try:
                profile = source_fn(profile, self.settings)
            except SourceUnavailableError as exc:
                logger.warning("Source %s unavailable: %s", name, exc)
                profile = profile.add_evidence(
                    EvidencePoint(
                        source=name,
                        data_type="source_status",
                        value="unavailable",
                        weight=0.0,
                        confidence=0.0,
                        raw_snippet=str(exc),
                    )
                )
            except RateLimitedError as exc:
                logger.warning("Source %s rate-limited: %s", name, exc)
                profile = profile.add_evidence(
                    EvidencePoint(
                        source=name,
                        data_type="source_status",
                        value="rate_limited",
                        weight=0.0,
                        confidence=0.0,
                        raw_snippet=str(exc),
                    )
                )
            except OsintProfilerError as exc:
                logger.error("Source %s error: %s", name, exc)
        return profile

    def _run_active_sources(self, profile: TargetProfile) -> TargetProfile:
        """Execute all registered active sources (requires authorization)."""
        if not self.allow_active:
            raise AuthenticationError("active_reconnaissance")
        for name, source_fn in self._sources.items():
            if not getattr(source_fn, "ACTIVE_ONLY", False):
                continue
            try:
                profile = source_fn(profile, self.settings)
            except AuthenticationError as exc:
                logger.warning("Active source %s requires auth: %s", name, exc)
        return profile

    def _correlate_and_score(self, profile: TargetProfile) -> TargetProfile:
        """Aggregate evidence and compute confidence scores."""
        from ..config.settings import WEIGHTS, TIERS

        evidence = profile.evidence
        if not evidence:
            return profile

        # Compute aggregate confidence: 1 - product(1 - weighted_score)
        aggregate = 1.0
        for point in evidence:
            if point.data_type == "source_status":
                continue
            aggregate *= 1.0 - point.weighted_score()
        aggregate = round(1.0 - aggregate, 4)

        # Determine classification using tier thresholds (descending order)
        if aggregate >= TIERS.confirmed[0]:
            classification = "confirmed"
        elif aggregate >= TIERS.high_candidate[0]:
            classification = "high_candidate"
        elif aggregate >= TIERS.moderate_candidate[0]:
            classification = "moderate_candidate"
        elif aggregate >= TIERS.weak_lead[0]:
            classification = "weak_lead"
        else:
            classification = "unresolved"

        # Extract real name candidates
        name_candidates = [
            e.value
            for e in evidence
            if e.data_type == "real_name" and e.confidence > 0.5
        ]

        # Extract location
        location_evidence = [
            e for e in evidence if e.data_type == "location"
        ]
        location = LocationIndicator()
        if location_evidence:
            best = max(location_evidence, key=lambda e: e.confidence)
            location = LocationIndicator(
                country=best.metadata.get("country"),
                region=best.metadata.get("region"),
                city=best.metadata.get("city"),
                basis=best.value,
                confidence=best.confidence,
            )

        return TargetProfile(
            alias=profile.alias,
            real_name_candidates=name_candidates,
            real_name_confidence=aggregate if name_candidates else 0.0,
            location=location,
            platforms=profile.platforms,
            evidence=evidence,
            infrastructure=profile.infrastructure,
            aggregate_confidence=aggregate,
            classification=classification,
            last_updated=profile.last_updated,
        )


__all__ = ["ProfilingEngine"]
