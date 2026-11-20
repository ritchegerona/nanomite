"""Wayback Machine passive data source.

Queries the Wayback Machine CDX API for archived snapshots of
target profile URLs. Returns snapshot timestamps and original URLs.
"""

from __future__ import annotations

import json
import logging
from typing import Any

from ..config.settings import Settings
from ..core.exceptions import SourceUnavailableError, ParseError
from ..core.models import EvidencePoint, TargetProfile

logger = logging.getLogger(__name__)

ACTIVE_ONLY = False


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Query Wayback Machine CDX API for archived pages."""
    alias = profile.alias
    logger.info("Querying Wayback Machine for %s", alias)

    candidate_urls = [
        f"facebook.com/{alias}",
        f"facebook.com/{alias.lower()}",
        f"instagram.com/{alias}/",
        f"instagram.com/{alias.lower()}/",
        f"t.me/{alias}",
        f"t.me/{alias.lower()}",
    ]

    found_any = False

    for original_url in candidate_urls:
        cdx_url = (
            "http://web.archive.org/cdx/search/cdx"
            f"?url={original_url}"
            "&output=json"
            "&limit=50"
            "&collapse=urlkey"
        )
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
                    cdx_url,
                ],
                capture_output=True,
                text=True,
                timeout=settings.network.request_timeout + 5,
            )
            if result.returncode != 0 or not result.stdout.strip():
                continue

            try:
                data = json.loads(result.stdout)
            except json.JSONDecodeError:
                logger.debug("Wayback returned non-JSON for %s", original_url)
                continue

            # CDX returns [header, row1, row2, ...]
            if len(data) <= 1:
                continue

            found_any = True
            snapshots = data[1:]

            for row in snapshots[:10]:  # Limit to first 10 snapshots
                timestamp = row[0] if len(row) > 0 else ""
                original = row[2] if len(row) > 2 else original_url
                status_code = row[3] if len(row) > 3 else ""
                mime = row[4] if len(row) > 4 else ""
                digest = row[5] if len(row) > 5 else ""

                profile = profile.add_evidence(
                    EvidencePoint(
                        source="wayback",
                        data_type="archived_snapshot",
                        value=f"{original} archived {timestamp} (HTTP {status_code})",
                        weight=0.40,
                        confidence=0.80,
                        raw_snippet=f"{timestamp} {original} {status_code} {mime}",
                        metadata={
                            "timestamp": timestamp,
                            "original_url": original,
                            "status_code": status_code,
                            "mime_type": mime,
                            "digest": digest,
                        },
                    )
                )

        except (subprocess.TimeoutExpired, OSError) as exc:
            logger.debug("Wayback query failed for %s: %s", original_url, exc)
            continue

    if not found_any:
        profile = profile.add_evidence(
            EvidencePoint(
                source="wayback",
                data_type="source_status",
                value="no_snapshots_found",
                weight=0.0,
                confidence=0.90,
                raw_snippet="No Wayback Machine snapshots found for any candidate URL",
            )
        )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
