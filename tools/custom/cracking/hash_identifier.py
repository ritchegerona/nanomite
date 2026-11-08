#!/usr/bin/env python3
"""Identify password-hash formats from their shape and prefix.

Does not crack anything — pair the output with hash_cracker.py. Use only on
hashes you are authorized to assess.

Usage:
  python3 hash_identifier.py '$2b$12$...'
  python3 hash_identifier.py --file hashes.txt --json
"""

import argparse
import json
import re
import sys

PREFIX_SIGNATURES = [
    (r"^\$2[abxy]?\$\d{2}\$", "bcrypt", "high"),
    (r"^\$argon2(id|i|d)\$", "argon2", "high"),
    (r"^\$scrypt\$", "scrypt", "high"),
    (r"^\$6\$", "sha512crypt", "high"),
    (r"^\$5\$", "sha256crypt", "high"),
    (r"^\$1\$", "md5crypt", "high"),
    (r"^\$apr1\$", "apache-md5 (apr1)", "high"),
    (r"^\$pbkdf2-sha256\$", "pbkdf2-sha256", "high"),
    (r"^pbkdf2_sha256\$", "django-pbkdf2-sha256", "high"),
    (r"^\$office\$", "MS Office 2007+", "high"),
    (r"^\$oldoffice\$", "MS Office 2003", "high"),
    (r"^\$pdf\$", "PDF encrypted", "high"),
    (r"^\$krb5", "Kerberos", "high"),
    (r"^\*[0-9A-Fa-f]{40}$", "MySQL 4.1+ native", "high"),
    (r"^0x[0-9A-Fa-f]{32}$", "MySQL 3.x", "medium"),
]

# (length, charset) -> candidate list
HEX_CANDIDATES = {
    8: [("CRC32", "medium")],
    13: [("DES crypt", "low")],
    16: [("half-MD5 / LM (half)", "low")],
    32: [("MD5", "high"), ("NTLM", "high"), ("MD4", "medium"), ("LM", "medium")],
    40: [("SHA1", "high"), ("RIPEMD-160", "medium"), ("MySQL 4.1+ (no prefix)", "medium")],
    56: [("SHA224", "high")],
    64: [("SHA256", "high"), ("Keccak-256", "medium"), ("BLAKE2s-256", "medium")],
    96: [("SHA384", "high")],
    128: [("SHA512", "high"), ("Whirlpool", "medium"), ("BLAKE2b-512", "medium")],
}

HEX_RE = re.compile(r"^[0-9A-Fa-f]+$")


def identify(value):
    value = value.strip()
    if not value:
        return []
    for pattern, name, conf in PREFIX_SIGNATURES:
        if re.match(pattern, value):
            return [{"type": name, "confidence": conf}]
    if HEX_RE.match(value):
        cands = HEX_CANDIDATES.get(len(value))
        if cands:
            return [{"type": n, "confidence": c} for n, c in cands]
        return [{"type": f"unknown hex ({len(value)} chars)", "confidence": "low"}]
    if len(value) == 13:
        return [{"type": "DES crypt", "confidence": "medium"}]
    return [{"type": "unknown", "confidence": "low"}]


def main():
    ap = argparse.ArgumentParser(description="password-hash format identifier")
    ap.add_argument("hash", nargs="?", help="hash string to identify")
    ap.add_argument("--file", help="file with one hash per line")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    values = []
    if args.hash:
        values.append(args.hash)
    if args.file:
        with open(args.file) as fh:
            values.extend(line.strip() for line in fh if line.strip() and not line.startswith("#"))
    if not values:
        ap.error("provide a hash or --file")

    results = []
    for v in values:
        results.append({"hash": v, "candidates": identify(v)})

    if args.json:
        print(json.dumps({"tool": "hash_identifier", "results": results}, indent=2))
    else:
        for r in results:
            best = r["candidates"][0]
            print(f"{r['hash'][:48]:<50} {best['type']:<28} ({best['confidence']})")
            for alt in r["candidates"][1:]:
                print(f"{'':<50} also: {alt['type']} ({alt['confidence']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
