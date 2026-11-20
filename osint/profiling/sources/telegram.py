"""Telegram passive OSINT gatherer."""

from __future__ import annotations
import re
import logging
from typing import List

import httpx

from ..config import Evidence, Target
from . import register_source

logger = logging.getLogger(__name__)

TELEGRAM_WEIGHT = 0.70
TELEGRAM_CONFIDENCE = 0.65


@register_source("telegram")
async def gather(target: Target) -> List[Evidence]:
    """Gather Telegram contact page and widget data."""
    evidence = []
    telegram_usernames = target.telegram_usernames
    aliases = target.all_aliases()
    
    all_usernames = list(dict.fromkeys(telegram_usernames + aliases))
    
    async with httpx.AsyncClient(
        timeout=15.0,
        headers={"User-Agent": "Mozilla/5.0 (compatible; OSINTProfiler/1.0)"},
        follow_redirects=True,
    ) as client:
        for username in all_usernames:
            clean_user = username.lstrip('@')
            
            # 1. Contact page
            contact_url = f"https://t.me/{clean_user}"
            try:
                resp = await client.get(contact_url)
                if resp.status_code == 200:
                    contact_data = _parse_contact_page(resp.text)
                    for key, value in contact_data.items():
                        evidence.append(Evidence(
                            source="telegram",
                            target_alias=target.alias,
                            data_type=f"contact:{key}",
                            raw_value=value,
                            confidence=TELEGRAM_CONFIDENCE,
                            weight=TELEGRAM_WEIGHT,
                            metadata={"url": contact_url, "username": clean_user},
                        ))
            except Exception as e:
                logger.debug("Telegram contact failed for %s: %s", clean_user, e)
            
            # 2. Embed widget (works for channels/groups)
            widget_url = f"https://t.me/s/{clean_user}"
            try:
                resp = await client.get(widget_url)
                if resp.status_code == 200:
                    widget_data = _parse_widget_page(resp.text)
                    for key, value in widget_data.items():
                        evidence.append(Evidence(
                            source="telegram",
                            target_alias=target.alias,
                            data_type=f"widget:{key}",
                            raw_value=value,
                            confidence=TELEGRAM_CONFIDENCE * 0.8,  # slightly lower
                            weight=TELEGRAM_WEIGHT * 0.8,
                            metadata={"url": widget_url, "username": clean_user},
                        ))
            except Exception as e:
                logger.debug("Telegram widget failed for %s: %s", clean_user, e)
    
    return evidence


def _parse_contact_page(html: str) -> dict:
    """Parse Telegram contact page for info."""
    data = {}
    
    # Check if it's a user, channel, or group
    if "tgme_page_title" in html:
        # Extract title
        match = re.search(r'class="tgme_page_title[^"]*"[^>]*>([^<]+)<', html)
        if match:
            data["title"] = match.group(1).strip()
    
    if "tgme_page_description" in html:
        match = re.search(r'class="tgme_page_description[^"]*"[^>]*>([^<]+)<', html)
        if match:
            data["description"] = match.group(1).strip()
    
    if "tgme_page_extra" in html:
        match = re.search(r'class="tgme_page_extra[^"]*"[^>]*>([^<]+)<', html)
        if match:
            data["extra"] = match.group(1).strip()
    
    # Check for "Channel not found" or "User not found"
    if "Channel not found" in html or "User not found" in html:
        data["status"] = "not_found"
    elif "If you have Telegram, you can contact" in html:
        data["status"] = "user_contact"
    elif "members" in html.lower() or "subscribers" in html.lower():
        data["status"] = "channel_or_group"
    
    return data


def _parse_widget_page(html: str) -> dict:
    """Parse Telegram embed widget page."""
    data = {}
    
    # Channel/group info from widget
    if "tgme_widget_message" in html:
        # Extract message previews
        matches = re.findall(r'class="tgme_widget_message_text[^"]*"[^>]*>([^<]+)<', html)
        if matches:
            data["recent_messages"] = matches[:5]
    
    # Channel title
    match = re.search(r'class="tgme_channel_info_header_title[^"]*"[^>]*>([^<]+)<', html)
    if match:
        data["channel_title"] = match.group(1).strip()
    
    # Member/subscriber count
    match = re.search(r'class="tgme_channel_info_header_members[^"]*"[^>]*>([^<]+)<', html)
    if match:
        data["member_count"] = match.group(1).strip()
    
    return data