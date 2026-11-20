"""Facebook passive OSINT gatherer."""

from __future__ import annotations
import re
import logging
from typing import List

import httpx

from ..config import Evidence, Target
from . import register_source

logger = logging.getLogger(__name__)

# Weight for Facebook evidence
FACEBOOK_WEIGHT = 0.90
FACEBOOK_CONFIDENCE = 0.85


@register_source("facebook")
async def gather(target: Target) -> List[Evidence]:
    """Gather Facebook OG metadata and page info."""
    evidence = []
    aliases = target.all_aliases()
    facebook_ids = target.facebook_ids
    
    # Try each alias as a Facebook page/profile
    async with httpx.AsyncClient(
        timeout=15.0,
        headers={"User-Agent": "Mozilla/5.0 (compatible; OSINTProfiler/1.0)"},
        follow_redirects=True,
    ) as client:
        for alias in aliases:
            # Try vanity URL
            urls = [
                f"https://www.facebook.com/{alias}",
                f"https://m.facebook.com/{alias}",
                f"https://mbasic.facebook.com/{alias}",
            ]
            
            for url in urls:
                try:
                    resp = await client.get(url)
                    if resp.status_code == 200:
                        og_data = _extract_og_tags(resp.text)
                        if og_data:
                            for key, value in og_data.items():
                                evidence.append(Evidence(
                                    source="facebook",
                                    target_alias=target.alias,
                                    data_type=f"og:{key}",
                                    raw_value=value,
                                    confidence=FACEBOOK_CONFIDENCE,
                                    weight=FACEBOOK_WEIGHT,
                                    metadata={"url": url, "alias": alias},
                                ))
                        # Also try to extract page ID
                        page_id = _extract_page_id(resp.text)
                        if page_id:
                            evidence.append(Evidence(
                                source="facebook",
                                target_alias=target.alias,
                                data_type="page_id",
                                raw_value=page_id,
                                confidence=FACEBOOK_CONFIDENCE,
                                weight=FACEBOOK_WEIGHT,
                                metadata={"url": url, "alias": alias},
                            ))
                except Exception as e:
                    logger.debug("Facebook fetch failed for %s: %s", url, e)
        
        # Try known Facebook IDs via graph.facebook.com
        for fb_id in facebook_ids:
            try:
                # Profile picture (often accessible)
                pic_url = f"https://graph.facebook.com/{fb_id}/picture?type=large&redirect=false"
                resp = await client.get(pic_url)
                if resp.status_code == 200:
                    data = resp.json()
                    pic_data = data.get("data", {})
                    if pic_data.get("url"):
                        evidence.append(Evidence(
                            source="facebook",
                            target_alias=target.alias,
                            data_type="profile_picture_url",
                            raw_value=pic_data["url"],
                            confidence=FACEBOOK_CONFIDENCE,
                            weight=FACEBOOK_WEIGHT,
                            metadata={"facebook_id": fb_id},
                        ))
                    if pic_data.get("is_silhouette") is not None:
                        evidence.append(Evidence(
                            source="facebook",
                            target_alias=target.alias,
                            data_type="has_custom_picture",
                            raw_value=str(not pic_data["is_silhouette"]).lower(),
                            confidence=FACEBOOK_CONFIDENCE,
                            weight=FACEBOOK_WEIGHT,
                            metadata={"facebook_id": fb_id},
                        ))
            except Exception as e:
                logger.debug("Graph API failed for %s: %s", fb_id, e)
    
    return evidence


def _extract_og_tags(html: str) -> dict:
    """Extract Open Graph meta tags from HTML."""
    og_data = {}
    # Match og:title, og:description, og:url, og:type, og:image, og:locale, etc.
    pattern = r'property="og:([^"]+)"\s+content="([^"]*)"'
    matches = re.findall(pattern, html)
    for key, value in matches:
        if value:
            og_data[key] = value
    return og_data


def _extract_page_id(html: str) -> str | None:
    """Extract Facebook page ID from page source."""
    # Look for page_id in various formats
    patterns = [
        r'"page_id"\s*:\s*"(\d+)"',
        r'"pageID"\s*:\s*"(\d+)"',
        r'page_id=(\d+)',
        r'"entity_id"\s*:\s*"(\d+)"',
    ]
    for pattern in patterns:
        match = re.search(pattern, html)
        if match:
            return match.group(1)
    return None