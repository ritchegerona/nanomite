#!/usr/bin/env python3
"""TLS posture audit: protocol support, negotiated cipher, certificate health.

Connects to a host:port with each protocol version as the only allowed one
and reports which complete, plus the certificate validity window and subject.
For hosts you own or are explicitly authorized to assess.

Usage:
  python3 tls_audit.py --host app.example.com
  python3 tls_audit.py --host 192.168.1.10 --port 8443 --json
"""

import argparse
import json
import socket
import ssl
import sys
import tempfile
import warnings
from datetime import datetime, timezone

warnings.filterwarnings("ignore", category=DeprecationWarning)

VERSIONS = [
    ("TLSv1", ssl.TLSVersion.TLSv1),
    ("TLSv1.1", ssl.TLSVersion.TLSv1_1),
    ("TLSv1.2", ssl.TLSVersion.TLSv1_2),
    ("TLSv1.3", ssl.TLSVersion.TLSv1_3),
]


def test_version(host, port, version_enum, timeout):
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    ctx.minimum_version = version_enum
    ctx.maximum_version = version_enum
    try:
        with socket.create_connection((host, port), timeout=timeout) as sock, \
                ctx.wrap_socket(sock, server_hostname=host) as tls:
            return {"supported": True, "version": tls.version(),
                    "cipher": tls.cipher()[0] if tls.cipher() else ""}
    except (OSError, ssl.SSLError) as e:
        return {"supported": False, "version": version_enum.name, "error": str(e)[:120]}


def certificate_info(host, port, timeout):
    """Fetch the cert PEM and decode it with the stdlib decoder."""
    pem = ssl.get_server_certificate((host, port), timeout=timeout)
    with tempfile.NamedTemporaryFile("w", suffix=".pem", delete=True) as tmp:
        tmp.write(pem)
        tmp.flush()
        cert = ssl._ssl._test_decode_cert(tmp.name)
    not_before = datetime.strptime(cert["notBefore"], "%b %d %H:%M:%S %Y %Z")
    not_after = datetime.strptime(cert["notAfter"], "%b %d %H:%M:%S %Y %Z")
    now = datetime.now()
    return {
        "subject": dict(x[0] for x in cert["subject"]),
        "issuer": dict(x[0] for x in cert["issuer"]),
        "sans": [e[1] for e in cert.get("subjectAltName", ())],
        "not_before": not_before.isoformat(),
        "not_after": not_after.isoformat(),
        "days_to_expiry": (not_after - now).days,
        "expired": (not_after - now).days < 0,
        "key_usage": cert.get("keyUsage", ""),
    }


def main():
    ap = argparse.ArgumentParser(description="TLS protocol + certificate audit")
    ap.add_argument("--host", required=True, help="hostname or IP")
    ap.add_argument("--port", type=int, default=443)
    ap.add_argument("--timeout", type=float, default=5.0)
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    protocols = {}
    for name, enum in VERSIONS:
        protocols[name] = test_version(args.host, args.port, enum, args.timeout)

    cert = None
    cert_error = None
    try:
        cert = certificate_info(args.host, args.port, args.timeout)
    except Exception as e:  # noqa: BLE001 - report, do not crash
        cert_error = str(e)[:200]

    result = {
        "tool": "tls_audit",
        "target": f"{args.host}:{args.port}",
        "scanned_at": datetime.now(timezone.utc).isoformat(),
        "protocols": protocols,
        "certificate": cert,
        "certificate_error": cert_error,
    }

    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(f"[*] {args.host}:{args.port}")
        for name, info in protocols.items():
            if info["supported"]:
                print(f"  {name:<8} SUPPORTED  {info['version']} / {info['cipher']}")
            else:
                print(f"  {name:<8} disabled   ({info['error']})")
        if cert:
            subj = cert["subject"]
            print(f"\n  subject : {subj.get('commonName', subj)}")
            print(f"  issuer  : {cert['issuer'].get('organizationName', cert['issuer'])}")
            print(f"  valid   : {cert['not_before']} -> {cert['not_after']}")
            flag = ""
            if cert["expired"]:
                flag = "  <-- EXPIRED"
            elif cert["days_to_expiry"] < 14:
                flag = "  <-- expires soon"
            print(f"  expiry  : {cert['days_to_expiry']} day(s) remaining{flag}")
            if cert["sans"]:
                print(f"  SANs    : {', '.join(cert['sans'])}")
        elif cert_error:
            print(f"\n  certificate: unavailable ({cert_error})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
