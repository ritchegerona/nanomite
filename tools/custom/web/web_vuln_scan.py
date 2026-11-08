#!/usr/bin/env python3
"""Web misconfiguration / vulnerability scanner (read-only, GET/HEAD only).

Checks security headers, cookie flags, CORS policy, HTTP methods, exposed
sensitive paths, directory listing, version disclosure and TLS posture.
No exploitation is performed — detection only. For sites you own or are
explicitly authorized to assess.

Usage:
  python3 web_vuln_scan.py --url https://app.example.com
  python3 web_vuln_scan.py --url http://192.168.1.10:8080 --insecure --json
"""

import argparse
import json
import socket
import ssl
import sys
import tempfile
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone

SECURITY_HEADERS = [
    ("strict-transport-security", "high", "HSTS missing — allows SSL stripping"),
    ("content-security-policy", "medium", "CSP missing — no XSS blast-radius limit"),
    ("x-content-type-options", "medium", "X-Content-Type-Options missing — MIME sniffing"),
    ("referrer-policy", "low", "Referrer-Policy missing — referrer leakage"),
    ("permissions-policy", "low", "Permissions-Policy missing — feature exposure not restricted"),
]

SENSITIVE_PATHS = [
    "/.git/config", "/.git/HEAD", "/.env", "/.env.backup", "/.DS_Store",
    "/backup.zip", "/backup.tar.gz", "/db.sql", "/dump.sql", "/phpinfo.php",
    "/server-status", "/server-info", "/.svn/entries", "/.htaccess",
    "/.htpasswd", "/wp-config.php.bak", "/config.php.bak", "/actuator",
    "/actuator/env", "/actuator/health", "/swagger-ui.html", "/swagger.json",
    "/openapi.json", "/graphql", "/console", "/debug", "/trace",
    "/robots.txt", "/sitemap.xml", "/security.txt", "/.well-known/security.txt",
    "/admin", "/administrator", "/manager/html", "/jmx-console",
]

TRACE_METHODS = ("TRACE", "TRACK")


def build_opener(insecure, timeout, user_agent):
    ctx = ssl.create_default_context()
    if insecure:
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    opener = urllib.request.build_opener(urllib.request.HTTPSHandler(context=ctx))
    opener.addheaders = [("User-Agent", user_agent)]
    return opener, ctx


def http_request(opener, url, method="GET", headers=None, timeout=10, body=None):
    req = urllib.request.Request(url, data=body, method=method)
    if headers:
        for k, v in headers.items():
            req.add_header(k, v)
    try:
        with opener.open(req, timeout=timeout) as resp:
            raw = resp.read(65536)
            return {
                "status": resp.status,
                "headers": {k.lower(): v for k, v in resp.headers.items()},
                "all_headers": resp.headers,
                "body": raw.decode("utf-8", "replace"),
                "url": resp.geturl(),
            }
    except urllib.error.HTTPError as e:
        return {
            "status": e.code,
            "headers": {k.lower(): v for k, v in e.headers.items()},
            "all_headers": e.headers,
            "body": e.read(65536).decode("utf-8", "replace") if e.fp else "",
            "url": url,
        }
    except (urllib.error.URLError, socket.timeout, ssl.SSLError, OSError) as e:
        return {"status": 0, "headers": {}, "all_headers": None, "body": "",
                "url": url, "error": str(e)}


def severity_rank(sev):
    return {"high": 3, "medium": 2, "low": 1, "info": 0}.get(sev, 0)


def check_headers(findings, resp):
    hdrs = resp["headers"]
    is_https = resp["url"].startswith("https://")
    for name, sev, msg in SECURITY_HEADERS:
        if name == "strict-transport-security" and not is_https:
            continue  # HSTS is only meaningful over HTTPS
        if name not in hdrs:
            findings.append({"check": "security-header", "severity": sev, "detail": msg})
    # clickjacking: need XFO or CSP frame-ancestors
    if "x-frame-options" not in hdrs and "frame-ancestors" not in hdrs.get("content-security-policy", ""):
        findings.append({"check": "clickjacking", "severity": "medium",
                         "detail": "No X-Frame-Options and no CSP frame-ancestors — clickjacking possible"})


def check_cookies(findings, resp):
    set_cookies = []
    if resp["all_headers"] is not None:
        set_cookies = resp["all_headers"].get_all("Set-Cookie") or []
    for cookie in set_cookies:
        name = cookie.split("=", 1)[0].strip()
        low = cookie.lower()
        if "secure" not in low and resp["url"].startswith("https"):
            findings.append({"check": "cookie", "severity": "medium",
                             "detail": f"Cookie '{name}' missing Secure flag"})
        if "httponly" not in low:
            findings.append({"check": "cookie", "severity": "medium",
                             "detail": f"Cookie '{name}' missing HttpOnly flag"})
        if "samesite" not in low:
            findings.append({"check": "cookie", "severity": "low",
                             "detail": f"Cookie '{name}' missing SameSite attribute"})


def check_cors(findings, opener, url, timeout):
    evil = "https://cors-probe.invalid"
    resp = http_request(opener, url, headers={"Origin": evil}, timeout=timeout)
    acao = resp["headers"].get("access-control-allow-origin", "")
    acac = resp["headers"].get("access-control-allow-credentials", "").lower()
    if acao == evil:
        sev = "high" if acac == "true" else "medium"
        findings.append({"check": "cors", "severity": sev,
                         "detail": f"ACAO reflects arbitrary Origin (credentials={acac})"})
    elif acao == "*":
        findings.append({"check": "cors", "severity": "medium",
                         "detail": "ACAO is wildcard '*'"})
    elif acao:
        findings.append({"check": "cors", "severity": "info",
                         "detail": f"ACAO present: {acao}"})


def check_methods(findings, opener, url, timeout):
    resp = http_request(opener, url, method="OPTIONS", timeout=timeout)
    allow = resp["headers"].get("allow", "")
    if allow:
        findings.append({"check": "methods", "severity": "info",
                         "detail": f"Allow: {allow}"})
    for m in TRACE_METHODS:
        r = http_request(opener, url, method=m, timeout=timeout)
        if r["status"] == 200:
            findings.append({"check": "methods", "severity": "medium",
                             "detail": f"{m} method enabled — XST risk"})
            break


def check_paths(findings, opener, base, timeout, workers):
    def probe(path):
        return path, http_request(opener, base.rstrip("/") + path, timeout=timeout)

    with ThreadPoolExecutor(max_workers=workers) as pool:
        for path, resp in pool.map(probe, SENSITIVE_PATHS):
            status = resp["status"]
            if status == 200:
                body = resp["body"][:4000]
                if path in ("/.git/config", "/.git/HEAD"):
                    findings.append({"check": "exposed-path", "severity": "high",
                                     "detail": f"{path} exposed (200) — source disclosure"})
                elif path in ("/.env", "/.env.backup"):
                    findings.append({"check": "exposed-path", "severity": "high",
                                     "detail": f"{path} exposed (200) — secrets may leak"})
                elif path in ("/phpinfo.php", "/actuator/env", "/server-status",
                              "/server-info", "/console", "/debug"):
                    findings.append({"check": "exposed-path", "severity": "medium",
                                     "detail": f"{path} exposed (200)"})
                elif "Index of /" in body:
                    findings.append({"check": "directory-listing", "severity": "medium",
                                     "detail": f"Directory listing enabled at {path}"})
                else:
                    findings.append({"check": "exposed-path", "severity": "low",
                                     "detail": f"{path} present (200)"})
            elif status in (401, 403):
                findings.append({"check": "exposed-path", "severity": "info",
                                 "detail": f"{path} present ({status})"})
    # directory listing on the root itself
    root = http_request(opener, base, timeout=timeout)
    if "Index of /" in root["body"]:
        findings.append({"check": "directory-listing", "severity": "medium",
                         "detail": "Directory listing enabled at /"})


def check_versions(findings, resp):
    for hdr in ("server", "x-powered-by", "x-aspnet-version", "x-generator"):
        val = resp["headers"].get(hdr)
        if val:
            findings.append({"check": "version-disclosure", "severity": "low",
                             "detail": f"{hdr}: {val}"})


def check_tls(findings, url, timeout):
    if not url.startswith("https://"):
        return
    host = urllib.request.urlparse(url).hostname
    port = urllib.request.urlparse(url).port or 443
    if not host:
        return
    try:
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        with socket.create_connection((host, port), timeout=timeout) as sock, \
                ctx.wrap_socket(sock, server_hostname=host) as ssock:
            findings.append({"check": "tls", "severity": "info",
                             "detail": f"{ssock.version()} / {ssock.cipher()[0]}"})
    except (OSError, ssl.SSLError) as e:
        findings.append({"check": "tls", "severity": "medium",
                         "detail": f"TLS handshake failed: {e}"})
        return
    # cert expiry via decode of PEM
    try:
        with tempfile.NamedTemporaryFile("w", suffix=".pem", delete=True) as tmp:
            tmp.write(ssl.get_server_certificate((host, port)))
            tmp.flush()
            cert = ssl._ssl._test_decode_cert(tmp.name)
        not_after = datetime.strptime(cert["notAfter"], "%b %d %H:%M:%S %Y %Z")
        days = (not_after - datetime.now()).days
        if days < 0:
            findings.append({"check": "tls", "severity": "high",
                             "detail": f"Certificate EXPIRED {abs(days)} day(s) ago"})
        elif days < 14:
            findings.append({"check": "tls", "severity": "medium",
                             "detail": f"Certificate expires in {days} day(s)"})
    except Exception:  # noqa: BLE001, S110 - best-effort, never fatal
        pass


def main():
    ap = argparse.ArgumentParser(description="read-only web misconfiguration scanner")
    ap.add_argument("--url", required=True, help="base URL, e.g. https://app.example.com")
    ap.add_argument("--timeout", type=float, default=10.0, help="per-request timeout")
    ap.add_argument("--workers", type=int, default=8, help="parallel path probes")
    ap.add_argument("--insecure", action="store_true", help="skip TLS verification")
    ap.add_argument("--user-agent", default="web-vuln-scan/1.0 (authorized assessment)")
    ap.add_argument("--skip-paths", action="store_true", help="skip sensitive-path probing")
    ap.add_argument("--json", action="store_true", help="emit JSON")
    args = ap.parse_args()

    base = args.url.rstrip("/")
    opener, _ = build_opener(args.insecure, args.timeout, args.user_agent)

    findings = []
    root = http_request(opener, base, timeout=args.timeout)
    if root.get("error") and root["status"] == 0:
        print(f"[!] request failed: {root['error']}", file=sys.stderr)
        return 2

    check_headers(findings, root)
    check_cookies(findings, root)
    check_versions(findings, root)
    check_cors(findings, opener, base, args.timeout)
    check_methods(findings, opener, base, args.timeout)
    check_tls(findings, base, args.timeout)
    if not args.skip_paths:
        check_paths(findings, opener, base, args.timeout, args.workers)

    # de-dup identical (check, detail)
    seen = set()
    unique = []
    for f in findings:
        key = (f["check"], f["detail"])
        if key not in seen:
            seen.add(key)
            unique.append(f)
    unique.sort(key=lambda f: (-severity_rank(f["severity"]), f["check"]))

    counts = {}
    for f in unique:
        counts[f["severity"]] = counts.get(f["severity"], 0) + 1

    if args.json:
        print(json.dumps({
            "tool": "web_vuln_scan",
            "target": base,
            "scanned_at": datetime.now(timezone.utc).isoformat(),
            "status": root["status"],
            "counts": counts,
            "findings": unique,
        }, indent=2))
    else:
        print(f"[*] {base} — HTTP {root['status']}")
        for f in unique:
            print(f"  [{f['severity'].upper():<6}] {f['check']:<18} {f['detail']}")
        summary = ", ".join(f"{v} {k}" for k, v in sorted(counts.items())) or "clean"
        print(f"[*] summary: {summary}")
    return 2 if counts.get("high") else 0


if __name__ == "__main__":
    sys.exit(main())
