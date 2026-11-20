"""Global configuration and constants for osint_profiler.

Frozen slotted dataclasses per DEVELOPMENT_STANDARDS.md.
All configuration is FOSS-only and environment-variable overridable.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Final


@dataclass(frozen=True, slots=True)
class ToolPaths:
    """Paths to external binaries used by the profiler."""

    whois: str = os.getenv("OSINT_WHOIS_PATH", "whois")
    exiftool: str = os.getenv("OSINT_EXIFTOOL_PATH", "exiftool")
    curl: str = os.getenv("OSINT_CURL_PATH", "curl")
    dig: str = os.getenv("OSINT_DIG_PATH", "dig")
    tor: str = os.getenv("OSINT_TOR_PATH", "tor")
    torsocks: str = os.getenv("OSINT_TORSOCKS_PATH", "torsocks")
    gobuster: str = os.getenv("OSINT_GOBUSTER_PATH", "gobuster")


@dataclass(frozen=True, slots=True)
class NetworkSettings:
    """Network and timeout configuration."""

    user_agent: str = (
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/120.0 Safari/537.36"
    )
    request_timeout: int = int(os.getenv("OSINT_REQUEST_TIMEOUT", "15"))
    max_retries: int = int(os.getenv("OSINT_MAX_RETRIES", "3"))
    rate_limit_delay: float = float(os.getenv("OSINT_RATE_LIMIT_DELAY", "1.0"))


@dataclass(frozen=True, slots=True)
class ScoringWeights:
    """Evidence weighting for confidence scoring.

    Weights are applied multiplicatively with source confidence
    in the aggregate confidence formula.
    """

    direct_real_name: float = 0.90  # Direct real name from title/bio
    corroborating_name: float = 0.75  # Name variant across platforms
    platform_id_match: float = 0.80  # Platform-assigned ID match
    email_correlation: float = 0.50  # Email alias match
    location_indicator: float = 0.60  # Geographic indicator (domain, language)
    weak_pattern: float = 0.30  # Similar naming pattern only
    no_data: float = 0.00  # No actionable data


@dataclass(frozen=True, slots=True)
class ConfidenceTiers:
    """Confidence score tiers and classifications."""

    confirmed: tuple[float, float] = (0.90, 1.00)
    high_candidate: tuple[float, float] = (0.70, 0.89)
    moderate_candidate: tuple[float, float] = (0.40, 0.69)
    weak_lead: tuple[float, float] = (0.10, 0.39)
    unresolved: tuple[float, float] = (0.00, 0.09)


@dataclass(frozen=True, slots=True)
class Settings:
    """Master settings container."""

    tools: ToolPaths
    network: NetworkSettings
    weights: ScoringWeights
    tiers: ConfidenceTiers
    workspace_root: str = os.getenv("OSINT_WORKSPACE_ROOT", "/Users/ritchegerona/nanomite")
    output_dir: str = os.getenv("OSINT_OUTPUT_DIR", "/Users/ritchegerona/nanomite/osint_profiler/output")

    def __post_init__(self) -> None:
        """Ensure output directory exists."""
        import pathlib

        pathlib.Path(self.output_dir).mkdir(parents=True, exist_ok=True)


def get_settings() -> Settings:
    """Return a singleton Settings instance."""
    return Settings(
        tools=ToolPaths(),
        network=NetworkSettings(),
        weights=ScoringWeights(),
        tiers=ConfidenceTiers(),
    )


# Module-level constants
TOOL_PATHS: Final = ToolPaths()
NETWORK: Final = NetworkSettings()
WEIGHTS: Final = ScoringWeights()
TIERS: Final = ConfidenceTiers()

__all__ = [
    "ToolPaths",
    "NetworkSettings",
    "ScoringWeights",
    "ConfidenceTiers",
    "Settings",
    "get_settings",
    "TOOL_PATHS",
    "NETWORK",
    "WEIGHTS",
    "TIERS",
]
