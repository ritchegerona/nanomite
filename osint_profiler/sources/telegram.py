"""Telegram passive data source.

Checks Telegram contact pages via t.me/username without authentication.
Contact pages expose minimal data (username availability, button text).
Full channel/message access requires Telegram client or bot token (active).
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

_TG_PATTERNS = {
    "og_title": r'property="og:title"\s+content="([^"]+)"',
    "og_description": r'property="og:description"\s+content="([^"]+)"',
    "tgme_title": r'<div class="tgme_page_title">([^<]+)</div>',
    "tgme_description": r'<div class="tgme_page_description">([^<]*)</div>',
    "tgme_extra": r'<div class="tgme_page_extra">([^<]*)</div>',
    "channel_not_found": r'class="tgme_page_message"[^>]*>\s*(Channel|Group|Chat)\s+(not found|doesn.t exist)',
}


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Gather Telegram data for the target alias."""
    alias = profile.alias
    logger.info("Gathering Telegram data for %s", alias)

    # Telegram usernames are case-insensitive; try exact and lowercase
    candidate_urls = [
        f"https://t.me/{alias}",
        f"https://t.me/{alias.lower()}",
        f"https://t.me/s/{alias}",
        f"https://t.me/s/{alias.lower()}",
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
            if result.returncode == 0 and result.stdout and "telegram" in result.stdout.lower():
                html = result.stdout
                fetched_url = url
                break
        except (subprocess.TimeoutExpired, OSError) as exc:
            logger.debug("curl failed for %s: %s", url, exc)
            continue

    if html is None:
        raise SourceUnavailableError(
            "telegram", "Contact page blocked, not found, or requires JS"
        )

    # Check if channel/group exists
    not_found = re.search(_TG_PATTERNS["channel_not_found"], html)
    if not_found:
        profile = profile.add_platform(
            PlatformPresence(
                platform="telegram",
                handle=alias,
                user_id=None,
                url=fetched_url,
                accessible=False,
                data_extracted={"status": "not_found"},
                notes="Telegram contact page indicates user/channel not found",
            )
        )
        profile = profile.add_evidence(
            EvidencePoint(
                source="telegram",
                data_type="source_status",
                value="not_found",
                weight=0.0,
                confidence=0.90,
                raw_snippet=not_found.group(0),
                url=fetched_url,
            )
        )
        return profile

    # Extract metadata
    og_title = re.search(_TG_PATTERNS["og_title"], html)
    og_desc = re.search(_TG_PATTERNS["og_description"], html)
    tgme_title = re.search(_TG_PATTERNS["tgme_title"], html)
    tgme_desc = re.search(_TG_PATTERNS["tgme_description"], html)
    tgme_extra = re.search(_TG_PATTERNS["tgme_extra"], html)

    title_value = (tgme_title or og_title).group(1).strip() if (tgme_title or og_title) else ""
    desc_value = (tgme_desc or og_desc).group(1).strip() if (tgme_desc or og_desc) else ""
    extra_value = tgme_extra.group(1).strip() if tgme_extra else ""

    # Platform presence
    profile = profile.add_platform(
        PlatformPresence(
            platform="telegram",
            handle=alias,
            user_id=None,
            url=fetched_url,
            accessible=True,
            data_extracted={
                "title": title_value,
                "description": desc_value,
                "extra": extra_value,
                "page_type": "contact" if "Send Message" in html else "channel",
            },
            notes="Contact page accessible; minimal profile data exposed without login",
        )
    )

    # Evidence: title/description may contain real name or location
    if title_value:
        profile = profile.add_evidence(
            EvidencePoint(
                source="telegram",
                data_type="profile_title",
                value=title_value,
                weight=0.50,
                confidence=0.60,
                raw_snippet=title_value,
                url=fetched_url,
            )
        )

    if desc_value:
        profile = profile.add_evidence(
            EvidencePoint(
                source="telegram",
                data_type="profile_description",
                value=desc_value,
                weight=0.50,
                confidence=0.60,
                raw_snippet=desc_value,
                url=fetched_url,
            )
        )

    if extra_value:
        profile = profile.add_evidence(
            EvidencePoint(
                source="telegram",
                data_type="profile_extra",
                value=extra_value,
                weight=0.40,
                confidence=0.50,
                raw_snippet=extra_value,
                url=fetched_url,
            )
        )

    # Note: Member lists, message history, profile photos require Telethon/client
    profile = profile.add_evidence(
        EvidencePoint(
            source="telegram",
            data_type="source_status",
            value="passive_only",
            weight=0.0,
            confidence=0.0,
            raw_snippet="Member list, message history, and profile photo require Telethon client or bot token",
            url=fetched_url,
        )
    )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
