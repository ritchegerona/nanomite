"""Domain-specific exceptions for osint_profiler.

All exceptions inherit from OsintProfilerError for consistent error handling.
"""

from __future__ import annotations


class OsintProfilerError(Exception):
    """Base exception for all osint_profiler errors."""

    def __init__(self, message: str, code: int = 1) -> None:
        super().__init__(message)
        self.message = message
        self.code = code


class SourceUnavailableError(OsintProfilerError):
    """Raised when a data source is unavailable or blocked."""

    def __init__(self, source: str, reason: str = "") -> None:
        message = f"Source '{source}' unavailable: {reason}" if reason else f"Source '{source}' unavailable"
        super().__init__(message, code=2)
        self.source = source
        self.reason = reason


class AuthenticationError(OsintProfilerError):
    """Raised when authenticated access is required but credentials are missing."""

    def __init__(self, platform: str) -> None:
        super().__init__(f"Authentication required for platform '{platform}'", code=3)
        self.platform = platform


class RateLimitedError(OsintProfilerError):
    """Raised when a source returns rate-limited responses."""

    def __init__(self, source: str, retry_after: int | None = None) -> None:
        detail = f" Retry after {retry_after}s" if retry_after else ""
        super().__init__(f"Rate limited by '{source}'{detail}", code=4)
        self.source = source
        self.retry_after = retry_after


class ParseError(OsintProfilerError):
    """Raised when response parsing fails."""

    def __init__(self, source: str, raw_sample: str = "") -> None:
        detail = f" (sample: {raw_sample[:100]}...)" if raw_sample else ""
        super().__init__(f"Failed to parse response from '{source}'{detail}", code=5)
        self.source = source
        self.raw_sample = raw_sample


class ToolNotFoundError(OsintProfilerError):
    """Raised when a required external binary is not on PATH."""

    def __init__(self, tool: str) -> None:
        super().__init__(f"Required tool '{tool}' not found on PATH", code=6)
        self.tool = tool


__all__ = [
    "OsintProfilerError",
    "SourceUnavailableError",
    "AuthenticationError",
    "RateLimitedError",
    "ParseError",
    "ToolNotFoundError",
]
