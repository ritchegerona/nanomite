"""Advanced, memory-efficient network device discovery tool.

Demonstrates:
- Frozen slotted dataclasses (low memory footprint)
- Generators for streaming results (no full list in RAM)
- Context managers for subprocess lifecycle
- Type hints, structured logging, and custom exceptions
"""

from __future__ import annotations

import logging
import re
import subprocess
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass

logger = logging.getLogger(__name__)

from .errors import AuditError, DeviceParseError


@dataclass(frozen=True, slots=True)
class NetworkDevice:
    """Immutable, memory-light representation of a network device."""

    ip: str
    mac: str
    interface: str | None = None


@contextmanager
def run_command(cmd: list[str]) -> Iterator[str]:
    """Context manager that runs a command and yields decoded stdout.

    Raises:
        AuditError: If the command returns a non-zero exit code.
    """
    logger.debug("Executing: %s", " ".join(cmd))
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        stdout, stderr = proc.communicate()
        if proc.returncode != 0:
            raise AuditError(
                f"Command failed (exit {proc.returncode}): {stderr.strip()}"
            )
        yield stdout
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()


_ARP_LINE_RE = re.compile(
    r"\(\s*(?P<ip>[\d.]+)\s*\)\s+"
    r"at\s+(?P<mac>[0-9a-fA-F:]{17})\s+"
    r"on\s+(?P<interface>\S+)"
)


def discover_devices(cmd: list[str] | None = None) -> Iterator[NetworkDevice]:
    """Yield NetworkDevice objects one at a time from ARP output.

    Uses a generator so callers can consume results incrementally without
    loading the entire table into memory.

    Args:
        cmd: Command to produce ARP-like output. Defaults to `arp -a`.

    Yields:
        NetworkDevice: Parsed device records.

    Raises:
        AuditError: If the underlying command fails.
    """
    command = cmd or ["arp", "-a"]

    with run_command(command) as output:
        for line_number, line in enumerate(output.splitlines(), start=1):
            if not line.strip():
                # Blank lines are noise, not malformed data.
                continue
            match = _ARP_LINE_RE.search(line)
            if not match:
                # Defensive: any non-empty, non-matching line indicates
                # malformed input rather than silently skipping data.
                raise DeviceParseError(
                    f"Failed to parse ARP line {line_number}: {line!r}"
                )
            yield NetworkDevice(
                ip=match.group("ip"),
                mac=match.group("mac").lower(),
                interface=match.group("interface"),
            )


def main(argv: list[str] | None = None) -> int:
    """CLI entry point for device discovery."""
    import argparse

    parser = argparse.ArgumentParser(
        description="Discover devices on the local network (ARP)."
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Enable debug logging.",
    )
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    count = 0
    try:
        for device in discover_devices():
            print(f"{device.ip:15}  {device.mac:17}  {device.interface or '-'}")
            count += 1
    except AuditError as exc:
        logger.error("Audit failed: %s", exc)
        return 1

    logger.info("Discovered %d device(s)", count)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())