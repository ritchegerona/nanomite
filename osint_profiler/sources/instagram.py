"""Instagram passive data source.

Extracts Open Graph metadata from public Instagram profile pages
without authentication. Full profile data (bio details, tagged
photos, location tags) requires an authenticated session.
"""

from __future__ import annotations

import logging
import re
from typing import Any

from ..config.settings import Settings
from ..core.exceptions import SourceUnavailableError, ParseError
from ..core.models import EvidencePoint, PlatformPresence, TargetProfile

logger = logging.getLogger(__name__)

ACTIVE_ONLY = False

_IG_OG_PATTERNS = {
    "username": r'"username"\s*:\s*"([^"]+)"',
    "full_name": r'"full_name"\s*:\s*"([^"]*)"',
    "biography": r'"biography"\s*:\s*"([^"]*)"',
    "follower_count": r'"edge_followed_by"\s*:\s*{\s*"count"\s*:\s*(\d+)',
    "following_count": r'"edge_follow"\s*:\s*{\s*"count"\s*:\s*(\d+)',
    "post_count": r'"edge_owner_to_timeline_media"\s*:\s*{\s*"count"\s*:\s*(\d+)',
    "is_private": r'"is_private"\s*:\s*(true|false)',
    "og_title": r'property="og:title"\s+content="([^"]+)"',
    "og_description": r'property="og:description"\s+content="([^"]+)"',
}


def _extract_ig_metadata(html: str) -> dict[str, Any]:
    """Extract Instagram metadata from page HTML or embedded JSON."""
    data: dict[str, Any] = {}
    for key, pattern in _IG_OG_PATTERNS.items():
        match = re.search(pattern, html)
        if match:
            value = match.group(1)
            if key in ("follower_count", "following_count", "post_count"):
                data[key] = int(value)
            elif key == "is_private":
                data[key] = value == "true"
            else:
                data[key] = value.strip()
    return data


def _parse_ig_title(title: str) -> tuple[str | None, str | None]:
    """Parse Instagram page title.

    Example: "ahmd osam (@ahmdosa) • Instagram photos and videos"
    Returns: ("ahmd osam", "ahmdosa")
    """
    match = re.match(r"(.+?)\s*\(@([^)]+)\)\s*•", title)
    if match:
        return match.group(1).strip(), match.group(2).strip()
    return None, None


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Gather Instagram data for the target alias."""
    alias = profile.alias
    logger.info("Gathering Instagram data for %s", alias)

    candidate_urls = [
        f"https://www.instagram.com/{alias}/",
        f"https://www.instagram.com/{alias.lower()}/",
    ]

    html = None
    fetched_url = None

    for url in candidate_urls:
        try:
            import subprocess

            result = subprocess.run(
                [
                    settings.tools.curl,
                    "-s",
                    "-L",
                    "-A",
                    settings.network.user_agent,
                    "--max-time",
                    str(settings.network.request_timeout),
                    url,
                ],
                capture_output=True,
                text=True,
                timeout=settings.network.request_timeout + 5,
            )
            if result.returncode == 0 and result.stdout and ("instagram" in result.stdout.lower() or "Instagram" in result.stdout):
                html = result.stdout
                fetched_url = url
                break
        except (subprocess.TimeoutExpired, OSError) as exc:
            logger.debug("curl failed for %s: %s", url, exc)
            continue

    if html is None:
        raise SourceUnavailableError(
            "instagram", "Profile page blocked, JS-walled, or not found"
        )

    meta = _extract_ig_metadata(html)

    # Platform presence
    username = meta.get("username") or alias
    is_private = meta.get("is_private", True)

    profile = profile.add_platform(
        PlatformPresence(
            platform="instagram",
            handle=username,
            user_id=None,
            url=fetched_url,
            accessible=not is_private,
            data_extracted={
                "full_name": meta.get("full_name", ""),
                "biography": meta.get("biography", ""),
                "follower_count": meta.get("follower_count", 0),
                "following_count": meta.get("following_count", 0),
                "post_count": meta.get("post_count", 0),
                "is_private": is_private,
            },
            notes="OG metadata extracted passively; private profiles yield minimal data",
        )
    )

    # Evidence: real name from full_name
    full_name = meta.get("full_name", "")
    if full_name and full_name.strip():
        og_title = meta.get("og_title", "")
        parsed_name, parsed_handle = _parse_ig_title(og_title)
        name_value = parsed_name or full_name
        profile = profile.add_evidence(
            EvidencePoint(
                source="instagram",
                data_type="real_name",
                value=name_value,
                weight=settings.weights.direct_real_name,
                confidence=0.75 if parsed_name else 0.50,
                raw_snippet=og_title or full_name,
                url=fetched_url,
                metadata={"username": username, "is_private": is_private},
            )
        )

    # Evidence: bio content (may contain email, location, other handles)
    biography = meta.get("biography", "")
    if biography:
        profile = profile.add_evidence(
            EvidencePoint(
                source="instagram",
                data_type="bio",
                value=biography,
                weight=0.60,
                confidence=0.80,
                raw_snippet=biography,
                url=fetched_url,
                metadata={"bio_length": len(biography)},
            )
        )

        # Extract email-like patterns from bio
        email_match = re.search(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}", biography)
        if email_match:
            profile = profile.add_evidence(
                EvidencePoint(
                    source="instagram",
                    data_type="email",
                    value=email_match.group(0),
                    weight=settings.weights.email_correlation,
                    confidence=0.85,
                    raw_snippet=biography,
                    url=fetched_url,
                )
            )

    # Evidence: follower metrics
    if meta.get("follower_count", 0) > 0:
        profile = profile.add_evidence(
            EvidencePoint(
                source="instagram",
                data_type="engagement",
                value=f"followers:{meta['follower_count']} following:{meta.get('following_count', 0)} posts:{meta.get('post_count', 0)}",
                weight=0.10,
                confidence=0.90,
                url=fetched_url,
            )
        )

    # Note: Location tags, tagged photos, stories require authenticated access
    profile = profile.add_evidence(
        EvidencePoint(
            source="instagram",
            data_type="source_status",
            value="passive_only",
            weight=0.0,
            confidence=0.0,
            raw_snippet="Location tags, tagged photos, and stories require authenticated session",
            url=fetched_url,
        )
    )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
