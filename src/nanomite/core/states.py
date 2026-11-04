"""Typestate markers for device and session lifecycle.

Each state is a distinct, uninstantiable marker class. By parameterizing
``NetworkDevice`` and ``Session`` over a state type, invalid transitions
become static type errors rather than runtime bugs.
"""

from __future__ import annotations

from typing import Protocol, TypeVar, runtime_checkable

StateT = TypeVar("StateT", bound="StateProtocol")


class StateProtocol(Protocol):
    """Base protocol for all typestate markers."""


@runtime_checkable
class Discovered(StateProtocol, Protocol):
    """Device has been discovered but not yet authenticated."""


@runtime_checkable
class Authenticated(StateProtocol, Protocol):
    """Device credentials have been verified."""


@runtime_checkable
class Ready(StateProtocol, Protocol):
    """Device is authenticated and authorized to execute commands."""


@runtime_checkable
class Closed(StateProtocol, Protocol):
    """Device/session is terminated. No further operations permitted."""


__all__ = [
    "Authenticated",
    "Closed",
    "Discovered",
    "Ready",
    "StateProtocol",
    "StateT",
]
