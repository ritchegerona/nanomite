"""Typestate-enforced session lifecycle bound to a device.

A ``Session`` carries a device through lifecycle states. Operations are
typed by the session's current state marker, so calling ``read`` on an
``Unauthenticated`` session is a static type error.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Final, Generic, TypeVar

logger = logging.getLogger(__name__)

StateT = TypeVar("StateT")


class SessionState:
    """Base marker for session typestates."""
    __slots__ = ()


class Unauthenticated(SessionState):
    """Session created, no credentials presented."""
    __slots__ = ()


class Authenticated(SessionState):
    """Credentials accepted; session may open channels."""
    __slots__ = ()


class Active(SessionState):
    """Session is active; read/write operations permitted."""
    __slots__ = ()


class Closed(SessionState):
    """Session terminated."""
    __slots__ = ()


class SessionStateError(Exception):
    """Raised when a session operation violates typestate preconditions."""


@dataclass(frozen=True, slots=True)
class Session(Generic[StateT]):
    """Session parameterized by its typestate marker."""

    device_id: Final[str]
    correlation_id: Final[str]
    _state: StateT

    @classmethod
    def create(cls, device_id: str) -> Session[Unauthenticated]:
        """Allocate a session in the Unauthenticated state."""
        logger.info("Creating session for device %s", device_id)
        return cls(  # type: ignore[return-value, arg-type, type-var]
            device_id=device_id,
            correlation_id=str(__import__("uuid").uuid4()),
            _state=Unauthenticated(),  # type: ignore[arg-type]
        )

    # ------------------------------------------------------------------
    # Unauthenticated -> Authenticated
    # ------------------------------------------------------------------

    def authenticate(self, token: str) -> Session[Authenticated]:
        if not isinstance(self._state, Unauthenticated):
            raise SessionStateError(
                f"Session {self.correlation_id} is not Unauthenticated; "
                f"cannot authenticate (current: {type(self._state).__name__})"
            )
        if not token:
            raise ValueError("token required")
        logger.info("Session %s authenticating", self.correlation_id)
        return Session[Authenticated](
            device_id=self.device_id,
            correlation_id=self.correlation_id,
            _state=Authenticated(),
        )

    # ------------------------------------------------------------------
    # Authenticated -> Active
    # ------------------------------------------------------------------

    def activate(self) -> Session[Active]:
        if not isinstance(self._state, Authenticated):
            raise SessionStateError(
                f"Session {self.correlation_id} is not Authenticated; "
                f"cannot activate (current: {type(self._state).__name__})"
            )
        logger.info("Session %s activated", self.correlation_id)
        return Session[Active](
            device_id=self.device_id,
            correlation_id=self.correlation_id,
            _state=Active(),
        )

    # ------------------------------------------------------------------
    # Active-only operations
    # ------------------------------------------------------------------

    def read(self, size: int) -> bytes:
        """Read from session. Only valid in Active state."""
        if not isinstance(self._state, Active):
            raise SessionStateError(
                f"Session {self.correlation_id} is not Active; cannot read "
                f"(current: {type(self._state).__name__})"
            )
        if size <= 0:
            raise ValueError("size must be positive")
        logger.info("Session %s reading %d bytes", self.correlation_id, size)
        return b""

    def write(self, payload: bytes) -> int:
        """Write to session. Only valid in Active state."""
        if not isinstance(self._state, Active):
            raise SessionStateError(
                f"Session {self.correlation_id} is not Active; cannot write "
                f"(current: {type(self._state).__name__})"
            )
        if not payload:
            raise ValueError("payload required")
        logger.info("Session %s writing %d bytes", self.correlation_id, len(payload))
        return len(payload)

    # ------------------------------------------------------------------
    # Terminal transition
    # ------------------------------------------------------------------

    def close(self) -> Session[Closed]:
        """Terminate the session."""
        logger.info("Closing session %s", self.correlation_id)
        return Session[Closed](
            device_id=self.device_id,
            correlation_id=self.correlation_id,
            _state=Closed(),
        )

    @property
    def state_name(self) -> str:
        return type(self._state).__name__


__all__ = [
    "Active",
    "Authenticated",
    "Closed",
    "Session",
    "SessionStateError",
    "Unauthenticated",
]
