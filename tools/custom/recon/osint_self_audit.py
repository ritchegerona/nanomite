#!/usr/bin/env python3
"""osint_self_audit.py - OSINT self-audit for your own identifiers.

Purpose: audit what is publicly exposed about YOU and emit a Markdown
(or JSON) report with a remediation checklist. Covers email, username,
phone, name+location, and domains/IPs you own.

Authorization note: self-audit of your own identifiers; authorized use
only. Never run this against another person without their consent. The
network sections touch only public, unauthenticated endpoints and never
scrape private data or logged-in content.

Usage:
    python3 osint_self_audit.py --email you@example.com
    python3 osint_self_audit.py --username octocat --domain example.com
    python3 osint_self_audit.py --name "Juan Dela Cruz" --location "Quezon City"
    python3 osint_self_audit.py --ip 203.0.113.10 --output report.md
    echo '{"username": "octocat", "format": "json"}' | python3 osint_self_audit.py --stdin

Environment:
    HIBP_API_KEY   Optional HaveIBeenPwned API key (https://haveibeenpwned.com/API/Key).
                   Without it the HIBP section is skipped; everything else works.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import dataclasses
import hashlib
import http.client
import json
import os
import re
import socket
import ssl
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

UA = "osint-self-audit/1.0 (+https://github.com; self-audit tool)"
TIMEOUT_DEFAULT = 10


# ----------------------------------------------------------------- helpers

def http_request(url, timeout, headers=None, method="GET", max_bytes=1_000_000):
    """Fail-soft HTTP request. Returns (status, body_text). Status is None on transport error."""
    req = urllib.request.Request(url, method=method)
    req.add_header("User-Agent", UA)
    req.add_header("Accept", "application/json, text/html;q=0.9, */*;q=0.8")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read(max_bytes + 1)
            status = resp.status
            charset = resp.headers.get_content_charset() or "utf-8"
            return status, body[:max_bytes].decode(charset, errors="replace")
    except urllib.error.HTTPError as e:
        try:
            body = e.read(max_bytes).decode("utf-8", errors="replace")
        except Exception:
            body = ""
        return e.code, body
    except (urllib.error.URLError, TimeoutError, socket.timeout, OSError, ValueError) as e:
        return None, f"ERROR: {e}"


@dataclasses.dataclass
class Section:
    key: str
    title: str
    status: str  # FOUND | CLEAN | UNKNOWN | ERROR | SKIPPED | INFO
    lines: list
    data: dict = dataclasses.field(default_factory=dict)

    def render_md(self):
        out = [f"## {self.title}", "", f"**Status:** {self.status}", ""]
        out.extend(f"- {ln}" for ln in self.lines)
        out.append("")
        return "\n".join(out)


def _note_error(section, exc):
    section.status = "ERROR"
    section.lines.append(f"section failed: {exc}")
    return section


# ------------------------------------------------------------------- email

def audit_email(email, timeout):
    sec = Section("email", f"Email - {email}", "UNKNOWN", [])
    try:
        address = email.strip().lower()

        # (a) HaveIBeenPwned (optional key)
        hibp_key = os.environ.get("HIBP_API_KEY", "").strip()
        hibp = {"status": "skipped"}
        if not hibp_key:
            hibp = {"status": "skipped", "note": "HIBP_API_KEY not set - skipped"}
            sec.lines.append("HIBP breach check: skipped (set HIBP_API_KEY to enable). "
                             "Free key: https://haveibeenpwned.com/API/Key")
        else:
            url = ("https://haveibeenpwned.com/api/v3/breachedaccount/"
                   + urllib.parse.quote(address, safe="") + "?truncateResponse=false")
            status, body = http_request(url, timeout, {"hibp-api-key": hibp_key})
            if status == 200:
                try:
                    breaches = json.loads(body)
                except json.JSONDecodeError:
                    breaches = []
                hibp = {"status": "breached", "count": len(breaches)}
                sec.status = "FOUND"
                sec.lines.append(f"HIBP: {len(breaches)} breach(es) found:")
                for b in sorted(breaches, key=lambda b: b.get("BreachDate", ""), reverse=True)[:10]:
                    classes = ", ".join(b.get("DataClasses", [])[:5])
                    sec.lines.append(
                        f"  - {b.get('Name')} ({b.get('BreachDate', '?')}): {classes}")
                if len(breaches) > 10:
                    sec.lines.append(f"  - ... and {len(breaches) - 10} more")
            elif status == 404:
                hibp = {"status": "clean"}
                sec.lines.append("HIBP: no breaches found for this address.")
            elif status in (401, 403):
                hibp = {"status": "error", "note": "bad API key"}
                sec.lines.append("HIBP: API key rejected (401/403) - check HIBP_API_KEY.")
            elif status == 429:
                hibp = {"status": "error", "note": "rate limited (429)"}
                sec.lines.append("HIBP: rate limited (429) - retry later.")
            else:
                hibp = {"status": "error", "note": f"HTTP {status}"}
                sec.lines.append(f"HIBP: unexpected response (HTTP {status}).")

        # (b) Gravatar public profile
        digest = hashlib.md5(address.encode("utf-8")).hexdigest()
        g_url = f"https://www.gravatar.com/avatar/{digest}?d=404"
        g_status, _ = http_request(g_url, timeout, method="HEAD")
        gravatar = {"profile_exists": None}
        if g_status == 200:
            gravatar = {"profile_exists": True, "url": f"https://www.gravatar.com/{digest}"}
            sec.lines.append(f"Gravatar: PUBLIC avatar/profile exists - "
                             f"https://www.gravatar.com/avatar/{digest}")
        elif g_status == 404:
            gravatar = {"profile_exists": False}
            sec.lines.append("Gravatar: no public avatar (404).")
        else:
            gravatar = {"profile_exists": None, "note": f"HTTP {g_status}"}
            sec.lines.append(f"Gravatar: unknown (HTTP {g_status}).")

        # (c) emailrep.io reputation (free, no key)
        er_url = "https://emailrep.io/" + urllib.parse.quote(address, safe="")
        er_status, er_body = http_request(er_url, timeout)
        emailrep = {"status": "error"}
        if er_status == 200:
            try:
                er = json.loads(er_body)
            except json.JSONDecodeError:
                er = {}
            details = er.get("details", {}) if isinstance(er.get("details"), dict) else {}
            emailrep = {
                "reputation": er.get("reputation"),
                "suspicious": er.get("suspicious"),
                "references": er.get("references"),
                "data_breach": details.get("data_breach"),
                "credentials_leaked": details.get("credentials_leaked"),
                "profiles": details.get("profiles"),
            }
            sec.lines.append(
                f"emailrep.io: reputation={er.get('reputation')}, "
                f"suspicious={er.get('suspicious')}, "
                f"references={er.get('references')}, "
                f"data_breach={details.get('data_breach')}, "
                f"credentials_leaked={details.get('credentials_leaked')}")
            profiles = details.get("profiles") or []
            if profiles:
                sec.lines.append(f"emailrep.io social profiles seen: {', '.join(map(str, profiles))}")
        else:
            sec.lines.append(f"emailrep.io: unavailable (HTTP {er_status}).")

        sec.data = {"hibp": hibp, "gravatar": gravatar, "emailrep": emailrep}
        if sec.status != "FOUND":
            sec.status = "INFO"
        return sec
    except Exception as exc:  # fail-soft
        return _note_error(sec, exc)


# ---------------------------------------------------------------- username

# name -> (url template, optional body marker meaning "account exists")
# marker=None: rely on HTTP status (>=400 absent, 200 present).
# marker=str: 200 AND marker in body => exists; 200 without marker => unknown.
USERNAME_SITES = {
    "GitHub": ("https://github.com/{u}", None),
    "GitLab": ("https://gitlab.com/{u}", None),
    "Bitbucket": ("https://bitbucket.org/{u}/", None),
    "Reddit": ("https://www.reddit.com/user/{u}/", None),
    "YouTube": ("https://www.youtube.com/@{u}", None),
    "Instagram": ("https://www.instagram.com/{u}/", None),
    "Medium": ("https://medium.com/@{u}", None),
    "Dev.to": ("https://dev.to/{u}", None),
    "Hacker News": ("https://news.ycombinator.com/user?id={u}", "created:"),
    "Keybase": ("https://keybase.io/{u}", None),
    "Pinterest": ("https://www.pinterest.com/{u}/", None),
    "Twitch": ("https://decapi.me/twitch/uptime/{u}", None),  # special-cased below
    "Spotify": ("https://open.spotify.com/user/{u}", None),
    "Vimeo": ("https://vimeo.com/{u}", None),
    "Behance": ("https://www.behance.net/{u}", None),
    "Dribbble": ("https://dribbble.com/{u}", None),
    "Steam": ("https://steamcommunity.com/id/{u}", None),
    "Telegram": ("https://t.me/{u}", None),
    "Mastodon": ("https://mastodon.social/@{u}", None),
    "About.me": ("https://about.me/{u}", None),
    "Linktree": ("https://linktr.ee/{u}", None),
    "Kaggle": ("https://www.kaggle.com/{u}", None),
    "npm": ("https://www.npmjs.com/~{u}", None),
    "PyPI": ("https://pypi.org/user/{u}/", None),
    "Docker Hub": ("https://hub.docker.com/u/{u}", None),
    "WordPress.com": ("https://{u}.wordpress.com", None),
    "Blogspot": ("https://{u}.blogspot.com", None),
    "Flickr": ("https://www.flickr.com/people/{u}/", None),
    "DeviantArt": ("https://www.deviantart.com/{u}", None),
    "Etsy": ("https://www.etsy.com/shop/{u}", None),
    "Patreon": ("https://www.patreon.com/{u}", None),
    "Substack": ("https://{u}.substack.com", None),
}
# SPAs that cannot be reliably auto-checked - reported as manual links only.
# X/Twitter, TikTok, SoundCloud, Threads.
USERNAME_MANUAL = {
    "X/Twitter": "https://x.com/{u}",
    "TikTok": "https://www.tiktok.com/@{u}",
    "SoundCloud": "https://soundcloud.com/{u}",
    "Threads": "https://www.threads.net/@{u}",
}

def _check_site(name, tmpl, marker, username, timeout):
    url = tmpl.format(u=urllib.parse.quote(username, safe=""))
    try:
        if name == "Twitch":
            # decapi.me returns plain text: "User not found" or e.g. "x days, ..."
            status, body = http_request(url, timeout)
            if status == 200 and "not found" not in body.lower():
                return name, url, "FOUND"
            if status == 200:
                return name, url, "absent"
            return name, url, "UNKNOWN"
        if name == "Hacker News":
            status, body = http_request(url, timeout)
            if status == 200 and "created:" in body:
                return name, url, "FOUND"
            return name, url, "absent" if status == 200 else "UNKNOWN"
        status, body = http_request(url, timeout, method="HEAD")
        if status in (405, 501) or status is None:
            status, body = http_request(url, timeout, method="GET")
        if status is None:
            return name, url, "UNKNOWN"
        if status >= 400:
            return name, url, "absent"
        if marker:
            return name, url, ("FOUND" if marker in body else "UNKNOWN")
        return name, url, "FOUND"
    except Exception:
        return name, url, "UNKNOWN"


def audit_username(username, timeout):
    sec = Section("username", f"Username - {username}", "UNKNOWN", [])
    try:
        found, unknown = [], []
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            futures = [pool.submit(_check_site, n, t, m, username, timeout)
                       for n, (t, m) in USERNAME_SITES.items()]
            for fut in concurrent.futures.as_completed(futures):
                name, url, verdict = fut.result()
                if verdict == "FOUND":
                    found.append((name, url))
                elif verdict == "UNKNOWN":
                    unknown.append((name, url))
        sec.status = "FOUND" if found else "CLEAN"
        if found:
            sec.lines.append(f"{len(found)} public profile(s) found:")
            for name, url in sorted(found):
                sec.lines.append(f"  - {name}: {url}")
        else:
            sec.lines.append("No public profiles found on checked platforms.")
        if unknown:
            sec.lines.append(f"{len(unknown)} check(s) inconclusive: {', '.join(n for n, _ in sorted(unknown))}")
        sec.lines.append("Manual check (SPAs that cannot be auto-verified):")
        for name, tmpl in sorted(USERNAME_MANUAL.items()):
            sec.lines.append(f"  - {name}: {tmpl.format(u=username)}")
        sec.data = {"found": [{"site": n, "url": u} for n, u in sorted(found)],
                    "unknown": [n for n, _ in sorted(unknown)],
                    "manual_links": [t.format(u=username) for t in USERNAME_MANUAL.values()]}
        return sec
    except Exception as exc:
        return _note_error(sec, exc)


# ------------------------------------------------------------------- phone

COUNTRY_CODES = [
    ("880", "Bangladesh"), ("852", "Hong Kong"), ("358", "Finland"), ("420", "Czechia"),
    ("971", "UAE"), ("966", "Saudi Arabia"), ("972", "Israel"), ("234", "Nigeria"),
    ("254", "Kenya"), ("977", "Nepal"), ("94", "Sri Lanka"), ("63", "Philippines"),
    ("65", "Singapore"), ("81", "Japan"), ("82", "South Korea"), ("86", "China"),
    ("91", "India"), ("92", "Pakistan"), ("62", "Indonesia"), ("60", "Malaysia"),
    ("66", "Thailand"), ("84", "Vietnam"), ("61", "Australia"), ("64", "New Zealand"),
    ("44", "United Kingdom"), ("49", "Germany"), ("33", "France"), ("34", "Spain"),
    ("39", "Italy"), ("31", "Netherlands"), ("32", "Belgium"), ("43", "Austria"),
    ("41", "Switzerland"), ("46", "Sweden"), ("47", "Norway"), ("45", "Denmark"),
    ("353", "Ireland"), ("351", "Portugal"), ("48", "Poland"), ("30", "Greece"),
    ("90", "Turkey"), ("7", "Russia/Kazakhstan"), ("55", "Brazil"), ("52", "Mexico"),
    ("54", "Argentina"), ("56", "Chile"), ("57", "Colombia"), ("51", "Peru"),
    ("27", "South Africa"), ("20", "Egypt"), ("886", "Taiwan"), ("1", "US/Canada/Caribbean"),
]

BROKERS = [
    ("TruePeopleSearch", "https://www.truepeoplesearch.com/removal"),
    ("Whitepages", "https://www.whitepages.com/suppression-requests"),
    ("Spokeo", "https://www.spokeo.com/ccpa"),
    ("BeenVerified", "https://www.beenverified.com/app/optout/search"),
    ("Radaris", "https://radaris.com/control/privacy"),
    ("Intelius/TruthFinder (PeopleConnect)", "https://suppression.peopleconnect.us/login"),
    ("FastPeopleSearch", "https://www.fastpeoplesearch.com/removal"),
    ("Nuwber", "https://nuwber.com/removal/link"),
    ("ThatsThem", "https://thatsthem.com/optout"),
    ("PeopleFinders", "https://www.peoplefinders.com/opt-out"),
]

def normalize_phone(raw):
    cleaned = re.sub(r"[^\d+]", "", raw.strip())
    if cleaned.startswith("00"):
        cleaned = "+" + cleaned[2:]
    digits = re.sub(r"\D", "", cleaned)
    country = None
    if cleaned.startswith("+"):
        for code, name in sorted(COUNTRY_CODES, key=lambda kv: -len(kv[0])):
            if digits.startswith(code):
                country = name
                break
    valid = 8 <= len(digits) <= 15
    e164 = "+" + digits if cleaned.startswith("+") else digits
    return e164, digits, country, valid


def broker_optouts():
    return [f"{name}: {url}" for name, url in BROKERS]


def audit_phone(phone, timeout):
    sec = Section("phone", f"Phone - {phone}", "INFO", [])
    try:
        e164, digits, country, valid = normalize_phone(phone)
        sec.lines.append(f"Normalized: {e164}")
        sec.lines.append(f"Country guess: {country or 'unknown (missing +country code?)'}")
        sec.lines.append(f"E.164 plausible: {'yes' if valid else 'NO - check the number'}")
        sec.lines.append("People-search listings to check and opt out of (search your own number):")
        for name, url in BROKERS:
            sec.lines.append(f"  - {name} opt-out: {url}")
        dork = urllib.parse.quote_plus(f'"{digits}"')
        sec.lines.append(f"Google dork for your number: https://www.google.com/search?q=site:truepeoplesearch.com+OR+site:whitepages.com+OR+site:spokeo.com%20{dork}")
        sec.lines.append("If your number appears: file broker opt-outs, ask carriers for a port-freeze/PIN, "
                         "and consider a credit freeze (Equifax/Experian/TransUnion) to blunt phishing.")
        sec.data = {"e164": e164, "digits": digits, "country": country, "valid": valid,
                    "optouts": broker_optouts()}
        return sec
    except Exception as exc:
        return _note_error(sec, exc)


# ------------------------------------------------------------ name/location

def audit_name(name, location, timeout):
    label = f"{name}, {location}" if location else name
    sec = Section("name", f"Name + Location - {label}", "INFO", [])
    try:
        terms = f'"{name}"' + (f' "{location}"' if location else "")
        dorks = [
            f"{terms} site:spokeo.com",
            f"{terms} site:whitepages.com",
            f"{terms} site:beenverified.com",
            f"{terms} site:radaris.com",
            f"{terms} site:truepeoplesearch.com",
            f"{terms} site:fastpeoplesearch.com",
            f"{terms} site:thatsthem.com",
            f"{terms} site:nuwber.com",
            f'{terms} (filetype:pdf resume OR cv OR curriculum)',
            f"{terms} site:linkedin.com",
            f"{terms} site:pipl.com",
        ]
        sec.lines.append("Google dork searches (open each, review what is yours):")
        for d in dorks:
            sec.lines.append(f"  - {d}\n    https://www.google.com/search?q={urllib.parse.quote_plus(d)}")
        if location:
            loc = urllib.parse.quote_plus(location)
            q = urllib.parse.quote_plus(name)
            sec.lines.append(f"TruePeopleSearch direct results: https://www.truepeoplesearch.com/results?name={q}&citystatezip={loc}")
        sec.lines.append("If you find yourself listed, file opt-outs (repeat quarterly - listings reappear):")
        for item in broker_optouts():
            sec.lines.append(f"  - {item}")
        sec.lines.append("Public resume/CV? Request removal from hosting sites or switch to private link sharing.")
        sec.data = {"dorks": dorks, "optouts": broker_optouts()}
        return sec
    except Exception as exc:
        return _note_error(sec, exc)


# ------------------------------------------------------------------ domain

def _dig(rrtype, domain, timeout):
    try:
        out = subprocess.run(["dig", "+time=3", "+tries=1", "+short", rrtype, domain],
                             capture_output=True, text=True, timeout=timeout, check=False)
        return [ln.strip() for ln in out.stdout.splitlines() if ln.strip()][:10]
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return None


def audit_domain(domain, timeout):
    sec = Section("domain", f"Domain - {domain}", "INFO", [])
    dd = domain.strip().lower()
    try:
        # RDAP
        status, body = http_request(f"https://rdap.org/domain/{urllib.parse.quote(dd, safe='')}",
                                    timeout, {"Accept": "application/rdap+json"})
        rdap_info = {}
        if status == 200:
            try:
                rd = json.loads(body)
            except json.JSONDecodeError:
                rd = {}
            events = {e.get("eventAction"): e.get("eventDate", "")[:10]
                      for e in rd.get("events", [])}
            registrar = None
            for ent in rd.get("entities", []):
                roles = ent.get("roles", [])
                if "registrar" in roles:
                    for item in (ent.get("vcardArray") or [None, []])[1]:
                        if item and item[0] == "fn":
                            registrar = item[3]
            ns = [n.get("ldhName") for n in rd.get("nameservers", []) if n.get("ldhName")]
            sec.lines.append(f"RDAP registrar: {registrar or 'unknown'}")
            sec.lines.append(f"Created: {events.get('registration', '?')} | "
                             f"Expires: {events.get('expiration', '?')} | "
                             f"Updated: {events.get('last changed') or events.get('last update of RDAP database', '?')}")
            if ns:
                sec.lines.append(f"Nameservers: {', '.join(ns[:8])}")
            if rd.get("status"):
                sec.lines.append(f"Status: {', '.join(rd['status'])}")
            rdap_info = {"registrar": registrar, "created": events.get("registration"),
                         "expires": events.get("expiration"), "nameservers": ns,
                         "status": rd.get("status")}
        else:
            sec.lines.append(f"RDAP: unavailable (HTTP {status}).")
            rdap_info = {"error": f"HTTP {status}"}

        # DNS via dig (+socket fallback)
        records = {}
        for rr in ("A", "AAAA", "MX", "NS", "TXT"):
            res = _dig(rr, dd, timeout)
            records[rr] = res if res is not None else []
        if not any(records.values()):
            try:
                infos = socket.getaddrinfo(dd, None)
                records["A"] = sorted({i[4][0] for i in infos if i[4][0]})[:10]
            except OSError:
                pass
        for rr, vals in records.items():
            if vals:
                sec.lines.append(f"{rr}: {', '.join(vals)}")

        # Certificate Transparency (crt.sh) - public certs often leak subdomains
        ct_status, ct_body = http_request(
            "https://crt.sh/?q=" + urllib.parse.quote("%." + dd, safe="") + "&output=json",
            timeout, max_bytes=2_000_000)
        subdomains = []
        if ct_status == 200:
            try:
                entries = json.loads(ct_body)
            except json.JSONDecodeError:
                entries = []
            seen = set()
            for e in entries:
                for nm in str(e.get("name_value", "")).splitlines():
                    nm = nm.strip().lower()
                    if nm.endswith(dd) and not nm.startswith("*."):
                        seen.add(nm)
            subdomains = sorted(seen)[:50]
            sec.lines.append(f"crt.sh subdomains seen on public certs ({len(subdomains)}, cap 50):")
            for s in subdomains:
                sec.lines.append(f"  - {s}")
            if len(seen) > 50:
                sec.lines.append(f"  - ... and {len(seen) - 50} more")
        else:
            sec.lines.append(f"crt.sh: unavailable (HTTP {ct_status}).")

        sec.data = {"rdap": rdap_info, "dns": records, "subdomains": subdomains}
        return sec
    except Exception as exc:
        return _note_error(sec, exc)


# ---------------------------------------------------------------------- ip

def audit_ip(ip, timeout):
    sec = Section("ip", f"IP - {ip}", "INFO", [])
    try:
        try:
            socket.inet_pton(socket.AF_INET, ip)
            af = "ipv4"
        except OSError:
            socket.inet_pton(socket.AF_INET6, ip)
            af = "ipv6"

        status, body = http_request(f"https://rdap.org/ip/{urllib.parse.quote(ip, safe='')}",
                                    timeout, {"Accept": "application/rdap+json"})
        rdap_info = {}
        if status == 200:
            try:
                rd = json.loads(body)
            except json.JSONDecodeError:
                rd = {}
            org = None
            for ent in rd.get("entities", []):
                for item in (ent.get("vcardArray") or [None, []])[1]:
                    if item and item[0] == "fn":
                        org = item[3]
                        break
                if org:
                    break
            sec.lines.append(f"RDAP network: {rd.get('name', '?')} ({rd.get('handle', '?')})")
            sec.lines.append(f"Organization: {org or 'unknown'} | Country: {rd.get('country', '?')}")
            if rd.get("cidr0_cidrs"):
                cidrs = [f"{c.get('v4prefix') or c.get('v6prefix')}/{c.get('length')}"
                         for c in rd["cidr0_cidrs"][:5] if (c.get("v4prefix") or c.get("v6prefix"))]
                sec.lines.append(f"CIDRs: {', '.join(cidrs)}")
            rdap_info = {"network": rd.get("name"), "org": org, "country": rd.get("country")}
        else:
            sec.lines.append(f"RDAP: unavailable (HTTP {status}).")

        try:
            rev = socket.gethostbyaddr(ip)
            sec.lines.append(f"Reverse DNS: {rev[0]}")
            rdap_info["reverse_dns"] = rev[0]
        except (OSError, socket.herror, socket.gaierror):
            sec.lines.append("Reverse DNS: none.")

        geo_status, geo_body = http_request(f"https://ipwho.is/{urllib.parse.quote(ip, safe='')}", timeout)
        geo = {}
        if geo_status == 200:
            try:
                geo = json.loads(geo_body)
            except json.JSONDecodeError:
                geo = {}
            sec.lines.append(
                f"Geolocation: {geo.get('city', '?')}, {geo.get('region', '?')}, "
                f"{geo.get('country', '?')} | ISP: {geo.get('isp', '?')} | AS{geo.get('asn', '?')}")
        else:
            sec.lines.append(f"Geolocation: unavailable (HTTP {geo_status}).")

        sec.data = {"af": af, "rdap": rdap_info, "geo": {k: geo.get(k) for k in
                                                          ("country", "region", "city", "isp", "asn")} if geo else {}}
        return sec
    except Exception as exc:
        return _note_error(sec, exc)


# --------------------------------------------------------------- report

def remediation_checklist(sections):
    breaches = []
    for s in sections:
        hibp = s.data.get("hibp") if isinstance(s.data, dict) else None
        if hibp and hibp.get("status") == "breached":
            breaches.append(s.title.split(" - ", 1)[-1])
    checklist = [
        "Change passwords on every breached account; use a unique password per site (password manager recommended).",
        "Enable 2FA everywhere it is offered - prefer an authenticator app over SMS.",
        "Subscribe to free breach alerts: https://haveibeenpwned.com/NotifyMe",
        "File data-broker opt-outs for every broker listed above; re-check quarterly (listings reappear).",
        "Request removal of your info from Google: https://support.google.com/websearch/troubleshooter/3111061 (\"Results about you\").",
        "Lock down social profiles found in this report: private accounts, strip location/employer/phone from bios.",
        "Phone exposure? Ask your carrier for a port-freeze/PIN; consider a credit freeze at Equifax, Experian and TransUnion.",
        "Use email aliases (plus-addressing or addy.io/SimpleLogin) for signups so breaches don't trace back to you.",
        "Resume/CV exposed? Switch to unlisted links or request removal from hosting sites.",
        "Re-run this audit quarterly and after any suspected compromise.",
    ]
    if breaches:
        checklist.insert(0, f"PRIORITY: {len(breaches)} email(s) appeared in breaches: {', '.join(breaches)} - act on breached-password steps first.")
    return checklist


def render_markdown(sections, checklist):
    now = datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")
    parts = [
        "# OSINT Self-Audit Report",
        "",
        f"Generated: {now}",
        "Scope: self-audit of your own identifiers. Findings are leads, not proof.",
        "",
    ]
    for s in sections:
        parts.append(s.render_md())
    parts.append("## REMEDIATION CHECKLIST")
    parts.append("")
    parts.extend(f"- [ ] {item}" for item in checklist)
    parts.append("")
    parts.append("---")
    parts.append("Scope: self-audit. This report queries only public, unauthenticated endpoints; "
                 "no credentials or private data were used. Findings are leads, not proof - verify before acting.")
    parts.append("")
    return "\n".join(parts)


def render_json(sections, checklist):
    now = datetime.now(timezone.utc).isoformat(timespec="seconds")
    return json.dumps({
        "generated": now,
        "scope": "self-audit",
        "sections": {s.key: {"title": s.title, "status": s.status,
                             "lines": s.lines, "data": s.data} for s in sections},
        "remediation": checklist,
    }, indent=2)


# --------------------------------------------------------------------- cli

def build_parser():
    p = argparse.ArgumentParser(
        description="OSINT self-audit of your own identifiers (email, username, phone, "
                    "name+location, domain/IP). Emits a Markdown or JSON report.",
        epilog="Self-audit only. Authorized use only - never audit identifiers you do not own.")
    p.add_argument("--email")
    p.add_argument("--username")
    p.add_argument("--phone")
    p.add_argument("--name")
    p.add_argument("--location")
    p.add_argument("--domain")
    p.add_argument("--ip")
    p.add_argument("--output", default="osint_self_audit_report.md",
                   help="report path (default: ./osint_self_audit_report.md); '-' for stdout")
    p.add_argument("--json", action="store_true", help="emit JSON instead of Markdown")
    p.add_argument("--timeout", type=int, default=TIMEOUT_DEFAULT, help="per-request timeout seconds (default 10)")
    p.add_argument("--stdin", action="store_true",
                   help="read a JSON job from stdin: {\"email\":..., \"username\":..., ..., \"format\":\"json\"|\"markdown\"}; prints result to stdout")
    return p


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.stdin:
        try:
            job = json.load(sys.stdin)
        except json.JSONDecodeError as e:
            print(f"ERROR: stdin is not valid JSON: {e}", file=sys.stderr)
            return 1
        if not isinstance(job, dict):
            print("ERROR: stdin job must be a JSON object", file=sys.stderr)
            return 1
        for key in ("email", "username", "phone", "name", "location", "domain", "ip"):
            if not getattr(args, key):
                val = job.get(key)
                if val:
                    setattr(args, key, str(val))
        if str(job.get("format", "")).lower() == "json":
            args.json = True

    provided = [v for v in (args.email, args.username, args.phone, args.name,
                            args.domain, args.ip) if v]
    if not provided:
        parser.error("provide at least one of --email/--username/--phone/--name/--domain/--ip")

    timeout = max(1, args.timeout)
    sections = []
    if args.email:
        sections.append(audit_email(args.email, timeout))
    if args.username:
        sections.append(audit_username(args.username, timeout))
    if args.phone:
        sections.append(audit_phone(args.phone, timeout))
    if args.name:
        sections.append(audit_name(args.name, args.location, timeout))
    if args.domain:
        sections.append(audit_domain(args.domain, timeout))
    if args.ip:
        sections.append(audit_ip(args.ip, timeout))

    checklist = remediation_checklist(sections)
    report = render_json(sections, checklist) if args.json else render_markdown(sections, checklist)

    if args.stdin or args.output == "-":
        print(report)
    else:
        with open(args.output, "w", encoding="utf-8") as fh:
            fh.write(report)
        print(f"report written to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
