"""Defacement archive passive OSINT gatherer."""

from __future__ import annotations
import logging
from typing import List

import httpx

from ..config import Evidence, Target
from . import register_source

logger = logging.getLogger(__name__)

DEFACEMENT_WEIGHT = 0.85
DEFACEMENT_CONFIDENCE = 0.80


@register_source("defacement")
async def gather(target: Target) -> List[Evidence]:
    """Query Zone-H notifier archive for defacer aliases."""
    evidence = []
    aliases = target.all_aliases()
    
    async with httpx.AsyncClient(
        timeout=15.0,
        headers={"User-Agent": "Mozilla/5.0 (compatible; OSINTProfiler/1.0)"},
        follow_redirects=True,
    ) as client:
        for alias in aliases:
            # Zone-H notifier
            url = f"https://zone-h.org/notifier/alias={alias}"
            try:
                resp = await client.get(url)
                if resp.status_code == 200:
                    data = _parse_zone_h_notifier(resp.text)
                    if data.get("count", 0) > 0:
                        evidence.append(Evidence(
                            source="defacement",
                            target_alias=target.alias,
                            data_type="zone_h_count",
                            raw_value=str(data["count"]),
                            confidence=DEFACEMENT_CONFIDENCE,
                            weight=DEFACEMENT_WEIGHT,
                            metadata={"alias": alias, "url": url, **data},
                        ))
                    # If defacer profile has info
                    if data.get("country"):
                        evidence.append(Evidence(
                            source="defacement",
                            target_alias=target.alias,
                            data_type="zone_h_country",
                            raw_value=data["country"],
                            confidence=DEFACEMENT_CONFIDENCE,
                            weight=DEFACEMENT_WEIGHT,
                            metadata={"alias": alias, "url": url},
                        ))
                    if data.get("email"):
                        evidence.append(Evidence(
                            source="defacement",
                            target_alias=target.alias,
                            data_type="zone_h_email",
                            raw_value=data["email"],
                            confidence=DEFACEMENT_CONFIDENCE,
                            weight=DEFACEMENT_WEIGHT,
                            metadata={"alias": alias, "url": url},
                        ))
            except Exception as e:
                logger.debug("Zone-H failed for %s: %s", alias, e)
    
    return evidence


def _parse_zone_h_notifier(html: str) -> dict:
    """Parse Zone-H notifier page for defacer info."""
    import re
    data = {"count": 0}
    
    # Check for empty result
    if "No defacements found" in html or "No notifications found" in html:
        data["count"] = 0
        return data
    
    # Try to find count
    match = re.search(r'(\d+)\s*deface', html, re.IGNORECASE)
    if match:
        data["count"] = int(match.group(1))
    
    # Try to extract country
    match = re.search(r'Country\s*:\s*([^<\n]+)', html, re.IGNORECASE)
    if match:
        data["country"] = match.group(1).strip()
    
    # Try to extract email
    match = re.search(r'Email\s*:\s*([^<\n]+)', html, re.IGNORECASE)
    if match:
        data["email"] = match.group(1).strip()
    
    return data