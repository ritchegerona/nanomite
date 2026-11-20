"""WHOIS passive data source.

Performs domain registration lookups for alias-derived domains.
Returns registrant information, creation dates, and name servers.
"""

from __future__ import annotations

import logging
import re
from typing import Any

from ..config.settings import Settings
from ..core.exceptions import SourceUnavailableError, ToolNotFoundError
from ..core.models import EvidencePoint, TargetProfile
from shutil import which

logger = logging.getLogger(__name__)

ACTIVE_ONLY = False


def _check_whois_available(tools) -> None:
    """Verify whois binary is on PATH."""
    if which(tools.whois) is None:
        raise ToolNotFoundError(tools.whois)


def _parse_whois(output: str) -> dict[str, str]:
    """Parse WHOIS output into key-value pairs."""
    fields: dict[str, str] = {}
    patterns = {
        "registrar": r"Registrar:\s*(.+)",
        "registrant_name": r"(?:Registrant Name|Registrant):\s*(.+)",
        "registrant_org": r"Registrant Organization:\s*(.+)",
        "registrant_email": r"Registrant Email:\s*(.+)",
        "registrant_phone": r"Registrant Phone:\s*(.+)",
        "registrant_country": r"Registrant Country:\s*(.+)",
        "creation_date": r"Creation Date:\s*(.+)",
        "expiration_date": r"Registry Expiry Date:\s*(.+)",
        "name_server": r"Name Server:\s*(.+)",
    }
    for key, pattern in patterns.items():
        match = re.search(pattern, output, re.IGNORECASE)
        if match:
            fields[key] = match.group(1).strip()
    return fields


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Gather WHOIS data for alias-derived domains."""
    alias = profile.alias
    logger.info("Gathering WHOIS data for %s", alias)

    _check_whois_available(settings.tools)

    # Generate candidate domains from alias
    candidates = [
        f"{alias}.ph",
        f"{alias}.com",
        f"{alias}.net",
        f"{alias}.org",
        f"{alias}.info",
        f"{alias.lower()}.ph",
        f"{alias.lower()}.com",
    ]

    found_any = False

    for domain in candidates:
        try:
            import subprocess

            result = subprocess.run(
                [settings.tools.whois, domain],
                capture_output=True,
                text=True,
                timeout=settings.network.request_timeout,
            )
            if result.returncode != 0 or not result.stdout.strip():
                continue

            output = result.stdout
            parsed = _parse_whois(output)

            if not parsed:
                # Domain exists but no parseable fields
                found_any = True
                profile = profile.add_evidence(
                    EvidencePoint(
                        source="whois",
                        data_type="domain_exists",
                        value=domain,
                        weight=0.30,
                        confidence=0.90,
                        raw_snippet=output[:200],
                    )
                )
                continue

            found_any = True
            registrar = parsed.get("registrar", "unknown")
            profile.infrastructure = {**profile.infrastructure, domain: registrar}

            # Evidence: registrant name (real name candidate)
            if parsed.get("registrant_name"):
                profile = profile.add_evidence(
                    EvidencePoint(
                        source="whois",
                        data_type="real_name",
                        value=parsed["registrant_name"],
                        weight=settings.weights.direct_real_name,
                        confidence=0.80,
                        raw_snippet=parsed["registrant_name"],
                        metadata={"domain": domain, "registrar": registrar},
                    )
                )

            # Evidence: registrant email
            if parsed.get("registrant_email"):
                profile = profile.add_evidence(
                    EvidencePoint(
                        source="whois",
                        data_type="email",
                        value=parsed["registrant_email"],
                        weight=settings.weights.email_correlation,
                        confidence=0.85,
                        metadata={"domain": domain},
                    )
                )

            # Evidence: location
            if parsed.get("registrant_country"):
                profile = profile.add_evidence(
                    EvidencePoint(
                        source="whois",
                        data_type="location",
                        value=parsed["registrant_country"],
                        weight=settings.weights.location_indicator,
                        confidence=0.70,
                        metadata={"country": parsed["registrant_country"], "domain": domain},
                    )
                )

            # Evidence: creation date
            if parsed.get("creation_date"):
                profile = profile.add_evidence(
                    EvidencePoint(
                        source="whois",
                        data_type="infrastructure",
                        value=f"{domain} created {parsed['creation_date']}",
                        weight=0.20,
                        confidence=0.90,
                        metadata={"domain": domain, "creation_date": parsed["creation_date"]},
                    )
                )

        except (subprocess.TimeoutExpired, OSError) as exc:
            logger.debug("WHOIS failed for %s: %s", domain, exc)
            continue

    if not found_any:
        profile = profile.add_evidence(
            EvidencePoint(
                source="whois",
                data_type="source_status",
                value="no_domains_found",
                weight=0.0,
                confidence=0.90,
                raw_snippet="No alias-derived domains registered",
            )
        )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
