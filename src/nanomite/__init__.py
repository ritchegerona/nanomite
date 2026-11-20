"""Nanomite toolkit: minimal-resource security and AI utilities.

Exposes the OSINT identity mapping & doxxing framework for authorized
security research, plus the core cracking/audit subsystems.
"""

__version__ = "0.1.0"

from .osint.identity_mapper import (
    EvidenceRecord,
    IdentityCandidate,
    IdentityMapper,
)
from .osint.social_enum import PLATFORM_URL_TEMPLATES, SocialEnumerator, SocialProfile
from .osint.email_harvest import EmailHarvester, EmailRecord
from .osint.metadata_extractor import MetadataExtractor, MetadataResult
from .osint.correlator import Correlator, IdentityProfile
from .osint.report_generator import ReportGenerator
from .osint.osint_orchestrator import run_osint_orchestrator
from .osint.osint_trigger import OsintTrigger, OsintTriggerResult
from .osint.agent_integration import (
    auto_run,
    is_osint_task,
    scan_for_osint,
    run_osint_pipeline,
)

__all__ = [
    "EvidenceRecord",
    "IdentityCandidate",
    "IdentityMapper",
    "SocialEnumerator",
    "SocialProfile",
    "PLATFORM_URL_TEMPLATES",
    "EmailHarvester",
    "EmailRecord",
    "MetadataExtractor",
    "MetadataResult",
    "Correlator",
    "IdentityProfile",
    "ReportGenerator",
    "OsintTrigger",
    "OsintTriggerResult",
    "is_osint_task",
    "scan_for_osint",
    "run_osint_pipeline",
    "run_osint_orchestrator",
    "auto_run",
]
