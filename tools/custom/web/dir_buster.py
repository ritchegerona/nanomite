#!/usr/bin/env python3
"""Web content discovery (directory/file brute force), read-only GET.

Probes a wordlist (+ optional extensions) against a base URL and reports
responses that differ from a wildcard baseline. For sites you own or are
explicitly authorized to assess.

Usage:
  python3 dir_buster.py --url http://target.example.com
  python3 dir_buster.py --url http://target/ --wordlist words.txt --ext php,html,bak
"""

import argparse
import json
import sys
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone

DEFAULT_WORDS = [
    "admin", "login", "dashboard", "panel", "console", "manager",
    "api", "v1", "v2", "graphql", "swagger", "docs", "documentation",
    "backup", "bak", "old", "tmp", "temp", "test", "dev", "staging",
    "config", "conf", "settings", "setup", "install", "installer",
    "upload", "uploads", "files", "static", "assets", "public", "private",
    "data", "db", "database", "sql", "dump", "export", "import",
    "logs", "log", "monitor", "status", "health", "metrics", "prometheus",
    "git", "svn", "env", "hidden", "secret", "secrets",
    "user", "users", "account", "accounts", "profile", "register",
    "search", "query", "report", "reports", "stats", "statistics",
    "cron", "job", "jobs", "queue", "worker", "task",
    "wp-admin", "wp-login.php", "wp-content", "administrator",
    "phpmyadmin", "pma", "myadmin", "adminer", "webmail",
    "mail", "email", "newsletter", "subscribe",
    "cart", "checkout", "shop", "store", "order", "orders",
    "debug", "trace", "error", "errors", "exception",
    "robots.txt", "sitemap.xml", "favicon.ico", "crossdomain.xml",
    "security.txt", ".well-known/security.txt", "index.php",
]

DEFAULT_EXTS = ["", ".php", ".html", ".txt", ".bak", ".old", ".json", ".xml", ".zip"]

FOLLOW_REDIRECTS = True


def make_opener(insecure, timeout, user_agent):
    ctx = __import__("ssl").create_default_context()
    if insecure:
        ctx.check_hostname = False
        ctx.verify_mode = __import__("ssl").CERT_NONE
    opener = urllib.request.build_opener(
        urllib.request.HTTPRedirectHandler() if FOLLOW_REDIRECTS else urllib.request.HTTPHandler()
    )
    # rebuild with https support
    opener = urllib.request.build_opener(urllib.request.HTTPSHandler(context=ctx))
    opener.addheaders = [("User-Agent", user_agent)]
    return opener


def probe(opener, url, timeout):
    req = urllib.request.Request(url, method="GET")
    try:
        with opener.open(req, timeout=timeout) as resp:
            body = resp.read(8192)
            return {"status": resp.status, "length": len(body),
                    "location": resp.headers.get("Location", ""), "url": resp.geturl()}
    except urllib.error.HTTPError as e:
        return {"status": e.code, "length": 0, "location": "", "url": url}
    except Exception:  # noqa: BLE001 - network noise, skip
        return None


def main():
    ap = argparse.ArgumentParser(description="web content discovery (read-only)")
    ap.add_argument("--url", required=True, help="base URL")
    ap.add_argument("--wordlist", help="one path per line (default: built-in)")
    ap.add_argument("--ext", default="", help="comma list of extensions (default: common set)")
    ap.add_argument("--timeout", type=float, default=8.0)
    ap.add_argument("--workers", type=int, default=16)
    ap.add_argument("--insecure", action="store_true")
    ap.add_argument("--min-length", type=int, default=0, help="ignore hits shorter than N bytes")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    base = args.url.rstrip("/")
    if args.ext == "":
        exts = DEFAULT_EXTS
    else:
        exts = ["" if e.strip() == "" else ("." + e.strip().lstrip(".")) for e in args.ext.split(",")]

    if args.wordlist:
        with open(args.wordlist) as fh:
            words = [w.strip().lstrip("/") for w in fh if w.strip() and not w.startswith("#")]
    else:
        words = DEFAULT_WORDS

    paths = []
    for w in words:
        for e in exts:
            p = f"/{w}{e}"
            if p not in paths:
                paths.append(p)

    opener = make_opener(args.insecure, args.timeout, "dir-buster/1.0 (authorized assessment)")

    # wildcard baseline: random path
    rand_path = f"/__dirbuster_{datetime.now().strftime('%H%M%S%f')}__"
    baseline = probe(opener, base + rand_path, args.timeout)
    base_status = baseline["status"] if baseline else 0
    base_len = baseline["length"] if baseline else 0

    if not args.json:
        print(f"[*] baseline: status={base_status} length={base_len} — probing {len(paths)} path(s)")

    hits = []

    def check(path):
        r = probe(opener, base + path, args.timeout)
        return path, r

    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        for path, r in pool.map(check, paths):
            if not r or r["status"] == 0:
                continue
            if r["status"] == base_status and abs(r["length"] - base_len) < 32:
                continue  # wildcard / soft-404
            if r["status"] in (404, 400, 405, 503, 500):
                continue
            if r["length"] < args.min_length:
                continue
            hits.append({"path": path, "status": r["status"],
                         "length": r["length"], "location": r["location"]})

    hits.sort(key=lambda h: (h["status"], h["path"]))

    if args.json:
        print(json.dumps({"tool": "dir_buster", "target": base,
                          "scanned_at": datetime.now(timezone.utc).isoformat(),
                          "baseline": {"status": base_status, "length": base_len},
                          "hits": hits}, indent=2))
    else:
        for h in hits:
            line = f"{h['status']:<4} {h['length']:>8}  {h['path']}"
            if h["location"]:
                line += f"  -> {h['location']}"
            print(line)
        print(f"[*] {len(hits)} hit(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
