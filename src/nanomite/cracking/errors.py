"""Domain exceptions for cracking operations.

Follows the fail-fast domain-exception pattern from DEVELOPMENT_STANDARDS.md.
"""

from __future__ import annotations

from nanomite.errors import NanomiteError


class CrackError(NanomiteError):
    """Base exception for all cracking-domain failures."""


class SessionNotEngaged(CrackError):
    """Raised when an operation is attempted outside an engaged session."""


class OutOfScope(CrackError):
    """Raised when a target or wordlist falls outside the authorized scope."""


class CapabilityInvalid(CrackError):
    """Raised when the capability token is missing, expired, or revoked."""


class HashUnsupported(CrackError):
    """Raised when the hash algorithm is not supported by this engine."""

