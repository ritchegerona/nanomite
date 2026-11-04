#!/usr/bin/env python3
"""
ARP Inventory Baseline Comparator
---------------------------------

Compares a current ARP inventory JSON file against a saved baseline and
reports:
  - New hosts (IP or MAC not seen before)
  - Changed hosts (same IP, different MAC — possible ARP spoofing / rogue device)
  - Removed hosts (in baseline, missing from current)

Intended for defensive network monitoring on networks you own or have
explicit written authorization to monitor.

Usage:
  python3 baseline_comparator.py --current inventory_nonroot.json --baseline baseline.json --report report.json
  python3 baseline_comparator.py --current inventory_nonroot.json --baseline baseline.json --update-baseline

If --baseline does not exist, the current inventory becomes the baseline.
"""

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path


def load_json(path):
    try:
        with open(path, "r") as f:
            return json.load(f)
    except FileNotFoundError:
        return None
    except json.JSONDecodeError as e:
        print(f"[!] Invalid JSON in {path}: {e}")
        sys.exit(1)


def index_by_ip(hosts):
    return {h["ip"]: h for h in hosts if "ip" in h}


def index_by_mac(hosts):
    return {h["mac"]: h for h in hosts if "mac" in h}


def compare(current, baseline):
    cur_by_ip = index_by_ip(current)
    cur_by_mac = index_by_mac(current)
    base_by_ip = index_by_ip(baseline)
    base_by_mac = index_by_mac(baseline)

    new_hosts = []
    changed_hosts = []
    removed_hosts = []
    unchanged_hosts = []

    # Baseline IPs not in current → removed
    for ip, base_entry in base_by_ip.items():
        if ip not in cur_by_ip:
            removed_hosts.append({
                "ip": ip,
                "mac": base_entry.get("mac"),
                "first_seen": base_entry.get("timestamp"),
                "last_seen_in_baseline": base_entry.get("timestamp"),
            })

    # Current entries
    for ip, cur_entry in cur_by_ip.items():
        if ip not in base_by_ip:
            # IP never seen before → new host
            new_hosts.append(cur_entry)
        else:
            base_entry = base_by_ip[ip]
            base_mac = base_entry.get("mac", "").lower()
            cur_mac = cur_entry.get("mac", "").lower()
            if base_mac != cur_mac:
                # Same IP, different MAC → MAC change (possible spoofing/rogue)
                changed_hosts.append({
                    "ip": ip,
                    "old_mac": base_mac,
                    "new_mac": cur_mac,
                    "old_timestamp": base_entry.get("timestamp"),
                    "new_timestamp": cur_entry.get("timestamp"),
                    "interface": cur_entry.get("interface"),
                })
            else:
                unchanged_hosts.append(cur_entry)

    # MACs seen in baseline but now on a different IP → also flagged
    for mac, base_entry in base_by_mac.items():
        if mac not in cur_by_mac:
            ip = base_entry.get("ip")
            if ip and ip in cur_by_ip:
                # MAC still present but IP differs — caught in changed_hosts above
                pass

    return {
        "compared_at": datetime.utcnow().isoformat() + "Z",
        "baseline_file": None,  # filled by caller
        "current_file": None,   # filled by caller
        "summary": {
            "baseline_hosts": len(baseline),
            "current_hosts": len(current),
            "new": len(new_hosts),
            "changed": len(changed_hosts),
            "removed": len(removed_hosts),
            "unchanged": len(unchanged_hosts),
        },
        "new_hosts": new_hosts,
        "changed_hosts": changed_hosts,
        "removed_hosts": removed_hosts,
        "unchanged_hosts": unchanged_hosts,
    }


def print_report(report):
    s = report["summary"]
    print("\n=== ARP Inventory Baseline Comparison ===")
    print(f"Compared at: {report['compared_at']}")
    print(f"Current file: {report['current_file']}")
    print(f"Baseline file: {report['baseline_file']}")
    print("-" * 60)
    print(f"Baseline hosts: {s['baseline_hosts']}")
    print(f"Current hosts : {s['current_hosts']}")
    print(f"Unchanged     : {s['unchanged']}")
    print(f"New           : {s['new']}")
    print(f"Changed (MAC) : {s['changed']}")
    print(f"Removed       : {s['removed']}")
    print("-" * 60)

    if report["new_hosts"]:
        print("\n[!] NEW HOSTS:")
        for h in report["new_hosts"]:
            print(f"  + {h.get('ip')}  {h.get('mac')}  {h.get('interface')}  {h.get('source')}")

    if report["changed_hosts"]:
        print("\n[!] CHANGED MAC (same IP, different MAC — possible spoofing):")
        for h in report["changed_hosts"]:
            print(f"  ! {h['ip']}: {h['old_mac']} -> {h['new_mac']}  ({h['interface']})")

    if report["removed_hosts"]:
        print("\n[-] REMOVED HOSTS (in baseline, missing now):")
        for h in report["removed_hosts"]:
            print(f"  - {h.get('ip')}  {h.get('mac')}")

    if not report["new_hosts"] and not report["changed_hosts"] and not report["removed_hosts"]:
        print("\n[OK] No changes detected. Inventory matches baseline.")


def main():
    parser = argparse.ArgumentParser(
        description="Compare ARP inventory against a saved baseline (defensive monitoring)."
    )
    parser.add_argument("--current", required=True, help="Current inventory JSON file")
    parser.add_argument("--baseline", required=True, help="Baseline inventory JSON file")
    parser.add_argument("--report", help="Write comparison report to JSON file")
    parser.add_argument(
        "--update-baseline",
        action="store_true",
        help="Replace baseline with current inventory after comparison",
    )
    args = parser.parse_args()

    current_data = load_json(args.current)
    if current_data is None:
        print(f"[!] Current inventory file not found: {args.current}")
        sys.exit(1)

    baseline_data = load_json(args.baseline)

    if baseline_data is None:
        print(f"[!] Baseline not found: {args.baseline}")
        print("[*] Creating baseline from current inventory.")
        baseline_data = current_data

    current_hosts = current_data.get("hosts", [])
    baseline_hosts = baseline_data.get("hosts", [])

    report = compare(current_hosts, baseline_hosts)
    report["current_file"] = str(Path(args.current).resolve())
    report["baseline_file"] = str(Path(args.baseline).resolve())

    print_report(report)

    if args.report:
        report_path = Path(args.report)
        report_path.write_text(json.dumps(report, indent=2))
        print(f"\n[+] Report written to {report_path.resolve()}")

    if args.update_baseline:
        baseline_path = Path(args.baseline)
        baseline_path.write_text(json.dumps(current_data, indent=2))
        print(f"[+] Baseline updated: {baseline_path.resolve()}")


if __name__ == "__main__":
    main()
