"""Certificate Transparency Log source (crt.sh).

Queries crt.sh for SSL certificates issued for alias-derived domains.
Certificate subject/organization fields may reveal real names or organizations.
"""

from __future__ import annotations

import json
import logging
import re
from typing import Any

from ..config.settings import Settings
from ..core.exceptions import SourceUnavailableError, ParseError
from ..core.models import EvidencePoint, TargetProfile

logger = logging.getLogger(__name__)

ACTIVE_ONLY = False


def gather(profile: TargetProfile, settings: Settings) -> TargetProfile:
    """Query crt.sh for certificates matching alias-derived domains."""
    alias = profile.alias
    logger.info("Querying Certificate Transparency logs for %s", alias)

    candidates = [
        f"%.{alias}.ph",
        f"%.{alias}.com",
        f"%.{alias}.net",
        f"%.{alias.lower()}.ph",
        f"%.{alias.lower()}.com",
    ]

    found_any = False

    for pattern in candidates:
        url = f"https://crt.sh/?q={pattern}&output=json"
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
            if result.returncode != 0 or not result.stdout.strip():
                continue

            try:
                certs = json.loads(result.stdout)
            except json.JSONDecodeError:
                # May return HTML if blocked
                if "Cloudflare" in result.stdout or "403" in result.stdout:
                    raise SourceUnavailableError("crt_sh", "Cloudflare blocked")
                continue

            if not certs:
                continue

            found_any = True

            for cert in certs[:20]:  # Limit to first 20 results
                common_name = cert.get("common_name", "")
                name_value = cert.get("name_value", "")
                issuer = cert.get("issuer_ca_name", "")
                entry_ts = cert.get("entry_timestamp", "")

                # Evidence: certificate domain
                profile = profile.add_evidence(
                    EvidencePoint(
                        source="crt_sh",
                        data_type="infrastructure",
                        value=f"Certificate for {common_name or name_value} issued by {issuer}",
                        weight=0.30,
                        confidence=0.80,
                        raw_snippet=f"{common_name or name_value} | {issuer} | {entry_ts}",
                        metadata={"common_name": common_name, "issuer": issuer, "timestamp": entry_ts},
                    )
                )

                # Check if subject/organization contains real name-like data
                # crt.sh doesn't always include organization, but check anyway
                org = cert.get("organization", "")
                if org and len(org) > 2:
                    profile = profile.add_evidence(
                        EvidencePoint(
                            source="crt_sh",
                            data_type="real_name",
                            value=org,
                            weight=settings.weights.direct_real_name,
                            confidence=0.60,
                            raw_snippet=org,
                            metadata={"domain": common_name or name_value},
                        )
                    )

        except (subprocess.TimeoutExpired, OSError) as exc:
            logger.debug("crt.sh query failed for %s: %s", pattern, exc)
            continue

    if not found_any:
        profile = profile.add_evidence(
            EvidencePoint(
                source="crt_sh",
                data_type="source_status",
                value="no_certificates_found",
                weight=0.0,
                confidence=0.90,
                raw_snippet="No certificates found for alias-derived domains",
            )
        )

    return profile


__all__ = ["gather", "ACTIVE_ONLY"]

gather.ACTIVE_ONLY = ACTIVE_ONLY
