"""Instagram passive OSINT gatherer."""

from __future__ import annotations
import json
import re
import logging
from typing import List

import httpx

from ..config import Evidence, Target
from . import register_source

logger = logging.getLogger(__name__)

INSTAGRAM_WEIGHT = 0.80
INSTAGRAM_CONFIDENCE = 0.75


@register_source("instagram")
async def gather(target: Target) -> List[Evidence]:
    """Gather Instagram OG metadata and profile info."""
    evidence = []
    aliases = target.all_aliases()
    instagram_usernames = target.instagram_usernames
    
    all_usernames = list(dict.fromkeys(aliases + instagram_usernames))
    
    async with httpx.AsyncClient(
        timeout=15.0,
        headers={"User-Agent": "Mozilla/5.0 (compatible; OSINTProfiler/1.0)"},
        follow_redirects=True,
    ) as client:
        for username in all_usernames:
            # Clean username
            clean_user = username.lstrip('@')
            urls = [
                f"https://www.instagram.com/{clean_user}/",
                f"https://www.instagram.com/{clean_user}/?__a=1&__d=dis",
            ]
            
            for url in urls:
                try:
                    resp = await client.get(url)
                    if resp.status_code == 200:
                        og_data = _extract_og_tags(resp.text)
                        if og_data:
                            for key, value in og_data.items():
                                evidence.append(Evidence(
                                    source="instagram",
                                    target_alias=target.alias,
                                    data_type=f"og:{key}",
                                    raw_value=value,
                                    confidence=INSTAGRAM_CONFIDENCE,
                                    weight=INSTAGRAM_WEIGHT,
                                    metadata={"url": url, "username": clean_user},
                                ))
                        
                        # Try to extract JSON data from page
                        json_data = _extract_shared_data(resp.text)
                        if json_data:
                            profile_data = _parse_profile_json(json_data)
                            for key, value in profile_data.items():
                                evidence.append(Evidence(
                                    source="instagram",
                                    target_alias=target.alias,
                                    data_type=f"profile:{key}",
                                    raw_value=str(value),
                                    confidence=INSTAGRAM_CONFIDENCE,
                                    weight=INSTAGRAM_WEIGHT,
                                    metadata={"url": url, "username": clean_user},
                                ))
                except Exception as e:
                    logger.debug("Instagram fetch failed for %s: %s", url, e)
    
    return evidence


def _extract_og_tags(html: str) -> dict:
    """Extract Open Graph meta tags from Instagram HTML."""
    og_data = {}
    pattern = r'property="og:([^"]+)"\s+content="([^"]*)"'
    matches = re.findall(pattern, html)
    for key, value in matches:
        if value:
            og_data[key] = value
    return og_data


def _extract_shared_data(html: str) -> dict | None:
    """Extract window._sharedData from Instagram page."""
    patterns = [
        r'window\._sharedData\s*=\s*({.*?});</script>',
        r'"ProfilePage":\s*(\[.*?\])',
    ]
    for pattern in patterns:
        match = re.search(pattern, html, re.DOTALL)
        if match:
            try:
                return json.loads(match.group(1))
            except json.JSONDecodeError:
                continue
    return None


def _parse_profile_json(data: dict) -> dict:
    """Parse Instagram profile data from shared JSON."""
    result = {}
    try:
        # Navigate the JSON structure
        if "entry_data" in data and "ProfilePage" in data["entry_data"]:
            profile = data["entry_data"]["ProfilePage"][0]["graphql"]["user"]
            result["username"] = profile.get("username", "")
            result["full_name"] = profile.get("full_name", "")
            result["biography"] = profile.get("biography", "")
            result["follower_count"] = profile.get("edge_followed_by", {}).get("count", 0)
            result["following_count"] = profile.get("edge_follow", {}).get("count", 0)
            result["post_count"] = profile.get("edge_owner_to_timeline_media", {}).get("count", 0)
            result["is_verified"] = profile.get("is_verified", False)
            result["is_private"] = profile.get("is_private", True)
            result["external_url"] = profile.get("external_url", "")
    except Exception:
        pass
    return result