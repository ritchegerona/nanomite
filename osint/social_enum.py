"""Social media username enumeration for authorized OSINT research.

Attempts to discover public profiles across common platforms using
headless browser checks and search-engine snippets. Does not scrape
private content or authenticate to any service.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class SocialProfile:
    platform: str
    username: str
    url: str
    display_name: Optional[str] = None
    bio: Optional[str] = None
    follower_count: Optional[int] = None
    is_private: Optional[bool] = None
    last_post_date: Optional[str] = None
    confidence: float = 0.5

    def to_dict(self) -> dict:
        return {
            "platform": self.platform,
            "username": self.username,
            "url": self.url,
            "display_name": self.display_name,
            "bio": self.bio,
            "follower_count": self.follower_count,
            "is_private": self.is_private,
            "last_post_date": self.last_post_date,
            "confidence": self.confidence,
        }


PLATFORM_URL_TEMPLATES = {
    "facebook": "https://www.facebook.com/{username}",
    "instagram": "https://www.instagram.com/{username}",
    "twitter": "https://x.com/{username}",
    "tiktok": "https://www.tiktok.com/@{username}",
    "linkedin": "https://www.linkedin.com/in/{username}",
    "github": "https://github.com/{username}",
    "tryhackme": "https://tryhackme.com/p/{username}",
    "telegram": "https://t.me/{username}",
}


class SocialEnumerator:
    """Enumerate social profiles for a given username across platforms."""

    def __init__(self, platforms: Optional[list[str]] = None):
        self.platforms = platforms or list(PLATFORM_URL_TEMPLATES.keys())

    def enumerate(self, username: str) -> list[SocialProfile]:
        results: list[SocialProfile] = []
        for platform in self.platforms:
            template = PLATFORM_URL_TEMPLATES.get(platform)
            if not template:
                continue
            url = template.format(username=username)
            # In a real deployment this would use web_fetch/browse to check
            # whether the URL resolves to a live profile. Here we record the
            # candidate URL for later verification via the tool layer.
            results.append(
                SocialProfile(
                    platform=platform,
                    username=username,
                    url=url,
                    confidence=0.3,  # candidate only; needs verification
                )
            )
        return results

    @staticmethod
    def extract_from_search_snippet(snippet: str, platform: str) -> Optional[SocialProfile]:
        """Parse a search-engine result snippet into a SocialProfile."""
        url_match = re.search(r"https?://[^\s]+", snippet)
        if not url_match:
            return None
        url = url_match.group(0).rstrip(".,)")
        username_match = re.search(r"/([^/?#]+)/?$", url)
        username = username_match.group(1) if username_match else username
        return SocialProfile(
            platform=platform,
            username=username,
            url=url,
            confidence=0.7,  # observed in search index
        )