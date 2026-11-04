"""Typestate-enforced network device.

Invalid state transitions are static type errors (via overloads + generics)
and runtime guard exceptions (DeviceStateError). A device cannot be executed
unless its type parameter is Ready; it cannot be authenticated unless its
type parameter is Discovered.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Final, Generic, Protocol, TypeVar

logger = logging.getLogger(__name__)

StateT = TypeVar("StateT", bound="StateProtocol")


class StateProtocol(Protocol):
    """Marker protocol for device typestates."""


class Discovered:
    """Device discovered but not authenticated."""
    __slots__ = ()


class Authenticated:
    """Credentials verified."""
    __slots__ = ()


class Ready:
    """Authorized to execute commands."""
    __slots__ = ()


class Closed:
    """Session terminated. No further operations permitted."""
    __slots__ = ()


class DeviceStateError(Exception):
    """Raised when an operation is attempted in an invalid typestate."""


@dataclass(frozen=True, slots=True)
class NetworkDevice(Generic[StateT]):
    """A network device parameterized by its typestate.

    The generic parameter StateT is the compile-time witness.
    The private _state field is the runtime witness.
    """

    id: Final[str]
    address: Final[str]
    _state: StateT

    @classmethod
    def discover(cls, device_id: str, address: str) -> NetworkDevice[Discovered]:
        """Create a device in the Discovered state."""
        logger.info("Discovering device %s at %s", device_id, address)
        return cls(id=device_id, address=address, _state=Discovered())  # type: ignore[return-value, arg-type, type-var]

    def authenticate(self, credentials: str) -> NetworkDevice[Authenticated]:
        if not isinstance(self._state, Discovered):
            raise DeviceStateError(
                f"Device {self.id} is not in Discovered state; "
                f"cannot authenticate (current: {type(self._state).__name__})"
            )
        if not credentials:
            raise ValueError("credentials required")
        logger.info("Authenticating device %s", self.id)
        return NetworkDevice[Authenticated](
            id=self.id,
            address=self.address,
            _state=Authenticated(),
        )

    def authorize(self, scope: str) -> NetworkDevice[Ready]:
        if not isinstance(self._state, Authenticated):
            raise DeviceStateError(
                f"Device {self.id} is not Authenticated; cannot authorize "
                f"(current: {type(self._state).__name__})"
            )
        if not scope:
            raise ValueError("scope required")
        logger.info("Authorizing device %s with scope %s", self.id, scope)
        return NetworkDevice[Ready](
            id=self.id,
            address=self.address,
            _state=Ready(),
        )

    def execute(self, command: str) -> str:
        """Execute a command. Only valid in the Ready state."""
        if not isinstance(self._state, Ready):
            raise DeviceStateError(
                f"Device {self.id} is not Ready; cannot execute commands "
                f"(current: {type(self._state).__name__})"
            )
        if not command:
            raise ValueError("command required")
        logger.info("Executing on device %s: %s", self.id, command)
        return f"result:{command}"

    def close(self) -> NetworkDevice[Closed]:
        """Terminate the device session."""
        logger.info("Closing device %s", self.id)
        return NetworkDevice[Closed](
            id=self.id,
            address=self.address,
            _state=Closed(),
        )

    @property
    def state_name(self) -> str:
        return type(self._state).__name__


__all__ = [
    "Authenticated",
    "Closed",
    "DeviceStateError",
    "Discovered",
    "NetworkDevice",
    "Ready",
    "StateProtocol",
    "StateT",
]
