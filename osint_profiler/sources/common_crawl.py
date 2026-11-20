"""Common Crawl Index passive data source.

Queries Common Crawl columnar index for archived pages containing
target aliases in URLs. Returns matching WARC records with timestamps.
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

# Common Crawl indexes - single recent index for performance
CC_INDEXES = [
    "CC-MAIN-2025-30",
]


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Query Common Crawl indexes for alias-derived URLs."""
    alias = profile.alias
    logger.info("Querying Common Crawl indexes for %s", alias)

    url_patterns = [
        f"*{alias}*",
        f"*{alias.lower()}*",
    ]

    found_any = False

    for index in CC_INDEXES:
        for pattern in url_patterns:
            cdx_url = (
                f"https://index.commoncrawl.org/{index}-index"
                f"?url={pattern}&output=json&filter=status:200&limit=20"
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

                # Common Crawl returns NDJSON (one JSON object per line)
                lines = result.stdout.strip().splitlines()
                if not lines:
                    continue

                found_any = True

                for line in lines[:10]:  # Limit per index
                    try:
                        record = json.loads(line)
                    except json.JSONDecodeError:
                        continue

                    if not isinstance(record, dict):
                        continue

                    url = record.get("url", "")
                    timestamp = record.get("timestamp", "")
                    filename = record.get("filename", "")
                    offset = record.get("offset", "")
                    length = record.get("length", "")
                    mime = record.get("mime", "")

                    profile = profile.add_evidence(
                        EvidencePoint(
                            source="common_crawl",
                            data_type="archived_url",
                            value=f"{url} crawled {timestamp}",
                            weight=0.40,
                            confidence=0.75,
                            raw_snippet=f"{timestamp} {url} {mime}",
                            metadata={
                                "url": url,
                                "timestamp": timestamp,
                                "warc_filename": filename,
                                "offset": offset,
                                "length": length,
                                "mime_type": mime,
                                "index": index,
                            },
                        )
                    )

            except (subprocess.TimeoutExpired, OSError) as exc:
                logger.debug("Common Crawl query failed for %s: %s", pattern, exc)
                continue

    if not found_any:
        profile = profile.add_evidence(
            EvidencePoint(
                source="common_crawl",
                data_type="source_status",
                value="no_archived_pages",
                weight=0.0,
                confidence=0.90,
                raw_snippet="No Common Crawl archived pages found for any alias pattern",
            )
        )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
