"""Tests for typestate enforcement in core modules.

Invalid transitions must raise runtime errors. Valid transitions must
return correctly typed successor objects.
"""

from __future__ import annotations

import pytest

from nanomite.core.device import (
    Authenticated,
    Closed,
    DeviceStateError,
    Discovered,
    NetworkDevice,
    Ready,
)
from nanomite.core.session import (
    Active,
    Session,
    SessionStateError,
    Unauthenticated,
)
from nanomite.core.session import (
    Authenticated as SessionAuthenticated,
)
from nanomite.core.session import (
    Closed as SessionClosed,
)

# -----------------------------------------------------------------------
# Device typestate tests
# -----------------------------------------------------------------------


def test_device_discover_returns_discovered_state() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    assert isinstance(dev._state, Discovered)
    assert dev.state_name == "Discovered"


def test_device_authenticate_transitions_to_authenticated() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    authed = dev.authenticate("secret")
    assert isinstance(authed._state, Authenticated)
    assert authed.state_name == "Authenticated"
    assert authed.id == dev.id
    assert authed.address == dev.address


def test_device_authorize_transitions_to_ready() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    authed = dev.authenticate("secret")
    ready = authed.authorize("admin")
    assert isinstance(ready._state, Ready)
    assert ready.state_name == "Ready"


def test_device_execute_only_in_ready_state() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    ready = dev.authenticate("secret").authorize("admin")
    result = ready.execute("show version")
    assert result == "result:show version"


def test_device_authenticate_rejects_non_discovered() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    authed = dev.authenticate("secret")
    with pytest.raises(DeviceStateError, match="not in Discovered state"):
        authed.authenticate("again")


def test_device_authorize_rejects_non_authenticated() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    with pytest.raises(DeviceStateError, match="not Authenticated"):
        dev.authorize("admin")


def test_device_execute_rejects_non_ready() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    authed = dev.authenticate("secret")
    with pytest.raises(DeviceStateError, match="not Ready"):
        authed.execute("show version")


def test_device_close_transitions_to_closed() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    closed = dev.authenticate("secret").authorize("admin").close()
    assert isinstance(closed._state, Closed)
    assert closed.state_name == "Closed"


def test_device_execute_after_close_raises() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    closed = dev.authenticate("secret").authorize("admin").close()
    with pytest.raises(DeviceStateError, match="not Ready"):
        closed.execute("show version")


def test_device_authenticate_rejects_empty_credentials() -> None:
    dev = NetworkDevice.discover("dev-1", "192.168.1.10")
    with pytest.raises(ValueError, match="credentials required"):
        dev.authenticate("")


# -----------------------------------------------------------------------
# Session typestate tests
# -----------------------------------------------------------------------


def test_session_create_returns_unauthenticated_state() -> None:
    sess = Session.create("dev-1")
    assert isinstance(sess._state, Unauthenticated)
    assert sess.state_name == "Unauthenticated"
    assert sess.device_id == "dev-1"


def test_session_authenticate_transitions_to_authenticated() -> None:
    sess = Session.create("dev-1")
    authed = sess.authenticate("token-123")
    assert isinstance(authed._state, SessionAuthenticated)
    assert authed.state_name == "Authenticated"
    assert authed.correlation_id == sess.correlation_id


def test_session_activate_transitions_to_active() -> None:
    sess = Session.create("dev-1")
    active = sess.authenticate("token-123").activate()
    assert isinstance(active._state, Active)
    assert active.state_name == "Active"


def test_session_read_write_only_in_active_state() -> None:
    sess = Session.create("dev-1")
    active = sess.authenticate("token-123").activate()
    assert active.read(16) == b""
    assert active.write(b"ping") == 4


def test_session_authenticate_rejects_non_unauthenticated() -> None:
    sess = Session.create("dev-1")
    authed = sess.authenticate("token-123")
    with pytest.raises(SessionStateError, match="not Unauthenticated"):
        authed.authenticate("again")


def test_session_activate_rejects_non_authenticated() -> None:
    sess = Session.create("dev-1")
    with pytest.raises(SessionStateError, match="not Authenticated"):
        sess.activate()


def test_session_read_rejects_non_active() -> None:
    sess = Session.create("dev-1")
    authed = sess.authenticate("token-123")
    with pytest.raises(SessionStateError, match="not Active"):
        authed.read(16)


def test_session_write_rejects_non_active() -> None:
    sess = Session.create("dev-1")
    authed = sess.authenticate("token-123")
    with pytest.raises(SessionStateError, match="not Active"):
        authed.write(b"data")


def test_session_close_transitions_to_closed() -> None:
    sess = Session.create("dev-1")
    closed = sess.authenticate("token-123").activate().close()
    assert isinstance(closed._state, SessionClosed)
    assert closed.state_name == "Closed"


def test_session_read_after_close_raises() -> None:
    sess = Session.create("dev-1")
    closed = sess.authenticate("token-123").activate().close()
    with pytest.raises(SessionStateError, match="not Active"):
        closed.read(16)


def test_session_authenticate_rejects_empty_token() -> None:
    sess = Session.create("dev-1")
    with pytest.raises(ValueError, match="token required"):
        sess.authenticate("")
