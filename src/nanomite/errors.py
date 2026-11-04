"""Custom exception hierarchy for nanomite modules."""


class NanomiteError(Exception):
    """Base exception for all nanomite errors."""


class AuditError(NanomiteError):
    """Raised when a network audit operation fails."""


class DeviceParseError(AuditError):
    """Raised when a device entry cannot be parsed from raw output."""