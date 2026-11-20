"""Facebook passive data source.

Attempts to extract Open Graph metadata from public Facebook pages
without authentication. Full profile data (About, admins, location)
requires an authenticated session (active source).
"""

from __future__ import annotations

import logging
import re
from typing import Any

from ..config.settings import Settings
from ..core.exceptions import SourceUnavailableError, ParseError
from ..core.models import EvidencePoint, PlatformPresence, TargetProfile

logger = logging.getLogger(__name__)

ACTIVE_ONLY = False  # Passive extraction possible; full data needs auth

_FB_OG_PATTERNS = {
    "page_name": r'property="og:title"\s+content="([^"]+)"',
    "page_id": r'property="fb:page_id"\s+content="([^"]+)"',
    "followers": r'property="og:description"\s+content="([^"]*followers[^"]*)"',
    "locale": r'property="og:locale"\s+content="([^"]+)"',
    "site_name": r'property="og:site_name"\s+content="([^"]+)"',
}


def _extract_og_metadata(html: str) -> dict[str, str]:
    """Extract Open Graph metadata from Facebook HTML."""
    data: dict[str, str] = {}
    for key, pattern in _FB_OG_PATTERNS.items():
        match = re.search(pattern, html)
        if match:
            data[key] = match.group(1).strip()
    return data


def _parse_facebook_title(title: str) -> tuple[str | None, str | None]:
    """Parse Facebook page title to extract real name and handle.

    Example title: "Ahmed Salar (@ahmdosa) • Facebook"
    Returns: ("Ahmed Salar", "ahmdosa")
    """
    match = re.match(r"(.+?)\s*\(@([^)]+)\)\s*•\s*Facebook", title)
    if match:
        return match.group(1).strip(), match.group(2).strip()
    return None, None


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Gather Facebook data for the target alias.

    Attempts passive OG extraction. Returns updated TargetProfile.
    Raises SourceUnavailableError if the page cannot be fetched.
    """
    alias = profile.alias
    logger.info("Gathering Facebook data for %s", alias)

    # Construct candidate URLs
    candidate_urls = [
        f"https://www.facebook.com/{alias}",
        f"https://www.facebook.com/{alias.lower()}",
        f"https://m.facebook.com/{alias}",
        f"https://mbasic.facebook.com/{alias}",
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
            if result.returncode == 0 and result.stdout and "Facebook" in result.stdout:
                html = result.stdout
                fetched_url = url
                break
        except (subprocess.TimeoutExpired, OSError) as exc:
            logger.debug("curl failed for %s: %s", url, exc)
            continue

    if html is None:
        raise SourceUnavailableError(
            "facebook", "All candidate URLs returned empty or blocked content"
        )

    # Extract OG metadata
    og_data = _extract_og_metadata(html)

    # Platform presence
    page_name = og_data.get("page_name", "")
    page_id = og_data.get("page_id", "")
    handle = None
    real_name = None

    if page_name:
        real_name, handle = _parse_facebook_title(page_name)
        if handle:
            profile = profile.add_platform(
                PlatformPresence(
                    platform="facebook",
                    handle=handle,
                    user_id=page_id or None,
                    url=fetched_url,
                    accessible=True,
                    data_extracted={"page_name": page_name, "page_id": page_id},
                    notes="OG metadata extracted passively",
                )
            )
        else:
            profile = profile.add_platform(
                PlatformPresence(
                    platform="facebook",
                    handle=alias,
                    user_id=page_id or None,
                    url=fetched_url,
                    accessible=True,
                    data_extracted={"page_name": page_name, "page_id": page_id},
                    notes="Title parse failed; handle assumed from alias",
                )
            )

    # Evidence: real name from title
    if real_name:
        profile = profile.add_evidence(
            EvidencePoint(
                source="facebook",
                data_type="real_name",
                value=real_name,
                weight=settings.weights.direct_real_name,
                confidence=0.85,
                raw_snippet=page_name,
                url=fetched_url,
                metadata={"handle": handle or alias, "page_id": page_id},
            )
        )

    # Evidence: location from language/locale
    locale = og_data.get("locale", "")
    if locale:
        profile = profile.add_evidence(
            EvidencePoint(
                source="facebook",
                data_type="location",
                value=locale,
                weight=settings.weights.location_indicator,
                confidence=0.50,
                raw_snippet=locale,
                url=fetched_url,
                metadata={"locale": locale},
            )
        )

    # Evidence: follower count if present
    followers = og_data.get("followers", "")
    if followers:
        profile = profile.add_evidence(
            EvidencePoint(
                source="facebook",
                data_type="engagement",
                value=followers,
                weight=0.10,
                confidence=0.70,
                raw_snippet=followers,
                url=fetched_url,
            )
        )

    # Note: Full profile data (admins, page transparency, contact info)
    # requires authenticated access and is marked as active-only.
    profile = profile.add_evidence(
        EvidencePoint(
            source="facebook",
            data_type="source_status",
            value="passive_only",
            weight=0.0,
            confidence=0.0,
            raw_snippet="Full profile data requires authenticated session",
            url=fetched_url,
        )
    )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
