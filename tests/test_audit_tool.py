"""Unit tests for nanomite.audit_tool."""

from __future__ import annotations

import pytest

from nanomite.audit_tool import NetworkDevice, discover_devices
from nanomite.errors import AuditError, DeviceParseError


class _FakeProc:
    """Minimal subprocess stub for context-manager testing."""

    def __init__(self, stdout: str, returncode: int = 0) -> None:
        self.stdout = stdout
        self.returncode = returncode

    def communicate(self) -> tuple[str, str]:
        return self.stdout, ""

    def poll(self) -> int | None:
        return self.returncode

    def kill(self) -> None:
        pass

    def wait(self) -> None:
        pass


def _make_output(devices: list[tuple[str, str, str]]) -> str:
    lines = []
    for ip, mac, iface in devices:
        lines.append(f"? ({ip}) at {mac} on {iface} [ether]")
    return "\n".join(lines)


def test_discover_devices_yields_generator() -> None:
    """discover_devices must return a generator/iterator, not a list."""
    result = discover_devices(
        cmd=["echo", _make_output([("10.0.0.1", "aa:bb:cc:dd:ee:ff", "eth0")])]
    )
    assert hasattr(result, "__next__")


def test_discover_devices_parses_multiple() -> None:
    output = _make_output([
        ("192.168.1.1", "00:11:22:33:44:55", "eth0"),
        ("192.168.1.2", "66:77:88:99:aa:bb", "wlan0"),
    ])
    devices = list(discover_devices(cmd=["echo", output]))
    assert len(devices) == 2
    assert devices[0] == NetworkDevice(
        ip="192.168.1.1", mac="00:11:22:33:44:55", interface="eth0"
    )
    assert devices[1] == NetworkDevice(
        ip="192.168.1.2", mac="66:77:88:99:aa:bb", interface="wlan0"
    )


def test_devices_are_immutable() -> None:
    dev = NetworkDevice(ip="1.2.3.4", mac="aa:bb:cc:dd:ee:ff")
    with pytest.raises(AttributeError):
        dev.ip = "9.9.9.9"


def test_bad_command_raises_audit_error(monkeypatch: pytest.MonkeyPatch) -> None:
    import nanomite.audit_tool as module

    def fake_popen(cmd, **kwargs):
        return _FakeProc(stdout="", returncode=1)

    monkeypatch.setattr(module.subprocess, "Popen", fake_popen)
    with pytest.raises(AuditError):
        list(discover_devices(cmd=["false"]))


def test_unparsable_line_raises_device_parse_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import nanomite.audit_tool as module

    def fake_popen(cmd, **kwargs):
        return _FakeProc(stdout="garbage line without match", returncode=0)

    monkeypatch.setattr(module.subprocess, "Popen", fake_popen)
    with pytest.raises(DeviceParseError):
        list(discover_devices(cmd=["true"]))


def test_empty_output_yields_nothing() -> None:
    devices = list(discover_devices(cmd=["echo", ""]))
    assert devices == []