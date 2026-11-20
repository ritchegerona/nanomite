"""Dark Web passive data source (plan-only).

Provides query templates and execution stubs for dark web search engines
and leak forums. Requires Tor network access (torsocks/tor binary) which
is not available in this environment by default.

All functions in this module are marked ACTIVE_ONLY=True because they
require either Tor connectivity or authenticated forum access.
"""

from __future__ import annotations

import logging
from typing import Any

from ..config.settings import Settings
from ..core.exceptions import SourceUnavailableError, ToolNotFoundError
from ..core.models import EvidencePoint, TargetProfile
from shutil import which

logger = logging.getLogger(__name__)

ACTIVE_ONLY = True  # Requires Tor or authenticated forum access

# Dark web search engine clear-web proxies (often blocked by Cloudflare)
DARKWEB_ENGINES = {
    "ahmia": "https://ahmia.fi/search/?q={query}",
    "dark_search": "https://darksearch.io/search?query={query}",
    "onion_land": "https://www.onionlandsearch.com/search?q={query}",
}

# Leak forum domains (typically require registration)
LEAK_FORUMS = {
    "breachforums": "https://breachforums.com",
    "nulled": "https://nulled.to",
    "exploit_in": "https://exploit.in",
    "xss_is": "https://xss.is",
}


def _check_tor_available(tools) -> None:
    """Verify Tor tooling is available."""
    missing = []
    for tool_name in ["tor", "torsocks"]:
        path = getattr(tools, tool_name, None)
        if path and which(path) is None:
            missing.append(tool_name)
    if missing:
        raise ToolNotFoundError(f"{', '.join(missing)} (Tor stack incomplete)")


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Execute dark web and leak forum searches.

    This function checks for Tor availability and constructs search URLs.
    Actual execution requires tor+torsocks which may not be installed.
    Returns the profile with evidence about source availability.
    """
    alias = profile.alias
    logger.info("Preparing dark web search for %s", alias)

    # Check Tor availability
    try:
        _check_tor_available(settings.tools)
    except ToolNotFoundError as exc:
        logger.warning("Dark web search skipped: %s", exc)
        profile = profile.add_evidence(
            EvidencePoint(
                source="darkweb",
                data_type="source_status",
                value="tor_unavailable",
                weight=0.0,
                confidence=0.0,
                raw_snippet=str(exc),
            )
        )
        return profile

    # Build search queries
    queries = [
        f'"{alias}" doxx',
        f'"{alias}" leaked',
        f'"{alias}" breach',
        f'"{alias}" real name',
        f'"{alias}" site:breachforums.com',
        f'"{alias}" site:nulled.to',
    ]

    found_any = False

    for query in queries:
        for engine_name, url_template in DARKWEB_ENGINES.items():
            url = url_template.format(query=query)
            try:
                import subprocess

                result = subprocess.run(
                    [
                        settings.tools.torsocks,
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
                    timeout=settings.network.request_timeout + 10,
                )
                if result.returncode != 0 or not result.stdout.strip():
                    continue

                found_any = True
                profile = profile.add_evidence(
                    EvidencePoint(
                        source=f"darkweb:{engine_name}",
                        data_type="search_query",
                        value=query,
                        weight=0.50,
                        confidence=0.60,
                        raw_snippet=result.stdout[:300],
                        url=url,
                    )
                )

            except (subprocess.TimeoutExpired, OSError) as exc:
                logger.debug("Dark web query failed for %s: %s", query, exc)
                continue

    if not found_any:
        profile = profile.add_evidence(
            EvidencePoint(
                source="darkweb",
                data_type="source_status",
                value="no_results",
                weight=0.0,
                confidence=0.80,
                raw_snippet="No dark web results found for any query",
            )
        )

    # Record leak forum targets for manual follow-up
    for forum_name, forum_url in LEAK_FORUMS.items():
        profile = profile.add_evidence(
            EvidencePoint(
                source=f"leak_forum:{forum_name}",
                data_type="manual_target",
                value=forum_url,
                weight=0.0,
                confidence=0.0,
                raw_snippet=f"Manual investigation required: search {alias} on {forum_name}",
                url=forum_url,
                metadata={"requires_registration": True, "requires_tor": True},
            )
        )

    return profile


__all__ = ["gather", "ACTIVE_ONLY", "DARKWEB_ENGINES", "LEAK_FORUMS"]
