#!/usr/bin/env python3
"""
Rogue Access Point Detection Script
------------------------------------
Defensive network-security tool. Scans a wireless interface for nearby APs
and compares them against an authorized allow-list. Flags:

  1. Unknown BSSIDs (completely unauthorized APs)
  2. Evil twins (authorized SSID broadcast by an unknown/unauthorized BSSID)
  3. SSID mismatch on a known BSSID (AP spoofing another legitimate AP)

Requirements:
  - Linux with wireless-tools (`iwlist`) installed
  - Root/sudo privileges (WiFi scanning usually requires them)
  - Wireless interface name (e.g. wlan0, wlp2s0)

Usage:
  1. Edit INTERFACE and AUTHORIZED_APS below.
  2. Run: sudo python3 rogue_ap_detector.py
  3. Review console output and rogue_ap_alerts.log
"""

import subprocess
import re
import sys
from datetime import datetime

# ---------------------------------------------------------------------------
# CONFIGURATION — EDIT THESE
# ---------------------------------------------------------------------------
INTERFACE = "wlan0"  # Your wireless interface name (ip link or iw dev)

# Mapping of authorized SSID -> BSSID (MAC). Populate from your own APs.
AUTHORIZED_APS = {
    # Example entries — replace with your real SSIDs/BSSIDs:
    # "MyHomeWiFi": "AA:BB:CC:DD:EE:FF",
    # "Office_GW":  "11:22:33:44:55:66",
}

ALERT_LOG = "rogue_ap_alerts.log"


# ---------------------------------------------------------------------------
# SCANNING
# ---------------------------------------------------------------------------
def scan_aps(interface: str):
    """Return list of dicts: {ssid, bssid, signal, security}"""
    try:
        result = subprocess.run(
            ["iwlist", interface, "scan"],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except FileNotFoundError:
        print("Error: `iwlist` not found. Install `wireless-tools`.")
        sys.exit(1)
    except subprocess.TimeoutExpired:
        print("Error: WiFi scan timed out.")
        sys.exit(1)

    cells = []
    current = {}

    for line in result.stdout.splitlines():
        line = line.strip()

        # New cell begins
        if "Cell " in line and "Address:" in line:
            if current:
                cells.append(current)
            current = {}
            m = re.search(r"Address:\s*([0-9A-Fa-f:]{17})", line)
            if m:
                current["bssid"] = m.group(1).upper()

        # SSID
        elif "ESSID:" in line:
            m = re.search(r'ESSID:"(.*)"', line)
            current["ssid"] = m.group(1) if m else "<hidden>"

        # Signal strength
        elif "Signal level=" in line:
            m = re.search(r"Signal level=(-?\d+)", line)
            if m:
                current["signal"] = int(m.group(1))

        # Encryption
        elif "Encryption key:" in line:
            current["security"] = "Encrypted" if "on" in line else "Open"

    if current:
        cells.append(current)

    return cells


# ---------------------------------------------------------------------------
# DETECTION LOGIC
# ---------------------------------------------------------------------------
def detect_rogues(aps, authorized):
    rogues = []

    authorized_bssids = {b.upper() for b in authorized.values()}

    for ap in aps:
        bssid = ap.get("bssid", "")
        ssid = ap.get("ssid", "")

        # Known BSSID but wrong SSID (AP impersonating a different authorized AP)
        if bssid in authorized_bssids:
            auth_ssid = [s for s, b in authorized.items() if b.upper() == bssid]
            if auth_ssid and auth_ssid[0] != ssid:
                rogues.append({**ap, "reason": "SSID mismatch on authorized BSSID"})

        # Authorized SSID seen on unknown BSSID -> possible evil twin
        elif ssid in authorized and bssid not in authorized_bssids:
            rogues.append({**ap, "reason": "Evil twin: authorized SSID, unknown BSSID"})

        # Completely unknown AP
        else:
            rogues.append({**ap, "reason": "Unknown AP"})

    return rogues


# ---------------------------------------------------------------------------
# ALERTING
# ---------------------------------------------------------------------------
def alert(rogues):
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

    if not rogues:
        print(f"[{timestamp}] No rogue APs detected.")
        return

    lines = [f"[{timestamp}] ROGUE AP ALERT — {len(rogues)} detected:"]
    for r in rogues:
        lines.append(
            f"  SSID={r.get('ssid')}  "
            f"BSSID={r.get('bssid')}  "
            f"Signal={r.get('signal')} dBm  "
            f"Security={r.get('security')}  "
            f"Reason={r.get('reason')}"
        )

    output = "\n".join(lines)
    print(output)

    with open(ALERT_LOG, "a") as f:
        f.write(output + "\n\n")


# ---------------------------------------------------------------------------
# MAIN
# ---------------------------------------------------------------------------
def main():
    if not AUTHORIZED_APS:
        print("Warning: AUTHORIZED_APS is empty. Edit the script with your known APs.")

    print(f"Scanning interface {INTERFACE} ...")
    aps = scan_aps(INTERFACE)
    print(f"Found {len(aps)} AP(s).")

    rogues = detect_rogues(aps, AUTHORIZED_APS)
    alert(rogues)


if __name__ == "__main__":
    main()
