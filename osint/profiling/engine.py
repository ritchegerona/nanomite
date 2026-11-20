"""Profiling engine: evidence correlation and confidence scoring."""

from __future__ import annotations
import logging
from typing import Dict, List, Optional, Set

from .config import (
    ConfidenceTier,
    Evidence,
    Profile,
    Target,
)
from .sources import gather_all

logger = logging.getLogger(__name__)

# Source weight multipliers (how much each source contributes to final score)
SOURCE_WEIGHTS = {
    "facebook": 1.0,
    "instagram": 0.9,
    "telegram": 0.7,
    "defacement": 0.85,
    "common_crawl": 0.6,
}

# Data type weight multipliers
TYPE_WEIGHTS = {
    "og:title": 1.0,
    "og:description": 0.8,
    "og:url": 0.7,
    "og:type": 0.5,
    "og:image": 0.6,
    "og:locale": 0.6,
    "page_id": 0.95,
    "profile_picture_url": 0.5,
    "has_custom_picture": 0.4,
    "profile:username": 0.9,
    "profile:full_name": 0.95,
    "profile:biography": 0.8,
    "profile:follower_count": 0.4,
    "profile:following_count": 0.3,
    "profile:post_count": 0.3,
    "profile:is_verified": 0.7,
    "profile:external_url": 0.6,
    "contact:title": 0.7,
    "contact:description": 0.6,
    "contact:extra": 0.5,
    "contact:status": 0.4,
    "widget:channel_title": 0.8,
    "widget:member_count": 0.5,
    "widget:recent_messages": 0.6,
    "zone_h_count": 0.8,
    "zone_h_country": 0.9,
    "zone_h_email": 0.95,
}


async def run_profiling(
    target: Target,
    sources: Optional[List[str]] = None,
    timeout: float = 30.0,
) -> Profile:
    """Run full profiling pipeline for a target."""
    logger.info("Starting profiling for target: %s", target.alias)
    
    # Gather evidence from all passive sources
    evidence = await gather_all(
        target=target,
        sources=sources,
        timeout=timeout,
    )
    
    # Correlate and score
    profile = _correlate_and_score(target, evidence)
    
    logger.info(
        "Profiling complete for %s: tier=%s, confidence=%.2f",
        target.alias, profile.tier.value, profile.aggregate_confidence
    )
    
    return profile


def _correlate_and_score(target: Target, evidence: List[Evidence]) -> Profile:
    """Correlate evidence and compute aggregate confidence."""
    
    # Group evidence by data type
    by_type: Dict[str, List[Evidence]] = {}
    for ev in evidence:
        by_type.setdefault(ev.data_type, []).append(ev)
    
    # Compute aggregate confidence using weighted formula
    # Aggregate = 1 - Product(1 - weighted_score_i)
    aggregate = 0.0
    for ev in evidence:
        ws = ev.weighted_score()
        # Apply source and type multipliers
        source_mult = SOURCE_WEIGHTS.get(ev.source, 1.0)
        type_mult = TYPE_WEIGHTS.get(ev.data_type, 1.0)
        adjusted_ws = ws * source_mult * type_mult
        aggregate = 1.0 - (1.0 - aggregate) * (1.0 - adjusted_ws)
    
    # Extract real name candidates from evidence
    real_name_candidates = _extract_real_names(evidence, target)
    
    # Build location profile
    location_profile = _build_location_profile(evidence, target)
    
    # Determine tier
    tier = ConfidenceTier.from_score(aggregate)
    
    return Profile(
        target=target,
        evidence=evidence,
        aggregate_confidence=round(aggregate, 4),
        tier=tier,
        real_name_candidates=real_name_candidates,
        location_profile=location_profile,
    )


def _extract_real_names(evidence: List[Evidence], target: Target) -> List[str]:
    """Extract and deduplicate real name candidates from evidence."""
    candidates = list(target.real_name_candidates)  # Start with seeds
    
    name_indicators = {
        "og:title", "profile:full_name", "contact:title", "widget:channel_title",
    }
    
    for ev in evidence:
        if ev.data_type in name_indicators and ev.confidence > 0.5:
            value = ev.raw_value.strip()
            # Clean up common patterns
            value = _clean_name_candidate(value)
            if value and len(value) > 2:
                candidates.append(value)
    
    # Deduplicate preserving order
    seen = set()
    unique = []
    for c in candidates:
        cl = c.lower()
        if cl not in seen:
            seen.add(cl)
            unique.append(c)
    
    return unique


def _clean_name_candidate(value: str) -> str:
    """Clean a potential real name from OG/title data."""
    # Remove handle suffixes like " (@handle)" or " | Facebook"
    import re
    value = re.sub(r'\s*\(@[^)]+\)', '', value)
    value = re.sub(r'\s*\|\s*Facebook.*$', '', value, flags=re.IGNORECASE)
    value = re.sub(r'\s*\|\s*Instagram.*$', '', value, flags=re.IGNORECASE)
    value = re.sub(r'\s*-\s*Facebook.*$', '', value, flags=re.IGNORECASE)
    value = value.strip()
    return value


def _build_location_profile(evidence: List[Evidence], target: Target) -> Dict[str, any]:
    """Build geographic profile from evidence."""
    location = {}
    
    # Start with seed indicators
    if target.location_indicators:
        location["seed_indicators"] = target.location_indicators
    
    location_evidence = []
    
    for ev in evidence:
        # Facebook locale
        if ev.data_type == "og:locale":
            locale = ev.raw_value
            location_evidence.append(("locale", locale, ev.confidence * ev.weight))
            if "_" in locale:
                lang, country = locale.split("_", 1)
                location["language"] = lang
                location["country_code"] = country
        
        # Facebook language
        if ev.data_type == "og:locale" and "fil" in ev.raw_value.lower():
            location["language"] = "fil"
            location["likely_country"] = "Philippines"
        
        # Zone-H country
        if ev.data_type == "zone_h_country":
            location_evidence.append(("zone_h", ev.raw_value, ev.confidence * ev.weight))
            location["country"] = ev.raw_value
        
        # Domain TLD
        if ev.data_type in ("domain", "og:url"):
            import re
            tld_match = re.search(r'\.([a-z]{2,3})(?:/|$)', ev.raw_value)
            if tld_match:
                tld = tld_match.group(1)
                location_evidence.append(("tld", tld, ev.confidence * ev.weight))
                if tld == "ph":
                    location["likely_country"] = "Philippines"
        
        # Handle/alias patterns
        if ev.data_type in ("contact:title", "contact:description", "widget:channel_title"):
            text = ev.raw_value.lower()
            if "ph" in text or "philippines" in text or "pinoy" in text:
                location_evidence.append(("keyword", "Philippines", ev.confidence * ev.weight))
                location["likely_country"] = "Philippines"
    
    # Compute location confidence
    if location_evidence:
        loc_score = 1.0
        for _, _, ws in location_evidence:
            loc_score = 1.0 - (1.0 - loc_score) * (1.0 - ws)
        location["confidence"] = round(loc_score, 4)
    else:
        location["confidence"] = 0.0
    
    return location