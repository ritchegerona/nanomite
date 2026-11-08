#!/usr/bin/env python3
"""Offline dictionary hash cracker with mutation rules (multiprocessing).

Supports raw digests (MD5/SHA1/SHA224/SHA256/SHA384/SHA512), NTLM (if the
platform exposes MD4), and crypt(3) formats (sha512crypt, sha256crypt,
md5crypt, bcrypt where libcrypt supports it). Pure stdlib.

Use only on hashes you own or are explicitly authorized to recover.

Usage:
  python3 hash_cracker.py --hash <digest> --format md5 --wordlist rockyou.txt
  python3 hash_cracker.py --hash '$6$...' --format sha512crypt --wordlist words.txt --rules basic
"""

import argparse
import hashlib
import multiprocessing as mp
import os
import sys
import time

try:
    import crypt
    HAS_CRYPT = True
except ImportError:
    HAS_CRYPT = False

RAW_FORMATS = {"md5", "sha1", "sha224", "sha256", "sha384", "sha512", "ntlm"}
CRYPT_PREFIXES = {"$1$": "md5crypt", "$5$": "sha256crypt", "$6$": "sha512crypt",
                  "$2a$": "bcrypt", "$2b$": "bcrypt", "$2y$": "bcrypt"}

LEET = {"a": ["a", "4", "@"], "e": ["e", "3"], "i": ["i", "1", "!"],
        "o": ["o", "0"], "s": ["s", "5", "$"], "t": ["t", "7"]}
COMMON_SUFFIXES = ["", "1", "12", "123", "1234", "12345", "!", "!!", "1!", "01",
                   "2020", "2021", "2022", "2023", "2024", "2025", "2026"]
COMMON_PREFIXES = ["", "1", "123", "!"]


def leet_variants(word):
    """Bounded leetspeak expansion (first substitution only per char)."""
    out = [word]
    for i, ch in enumerate(word.lower()):
        if ch in LEET:
            for sub in LEET[ch][1:]:
                out.append(word[:i] + sub + word[i + 1:])
    return out


def mutate(word, rules):
    """Yield candidate passwords for one base word."""
    base = word.strip()
    if not base:
        return
    yield base
    if rules == "none":
        return
    variants = {base, base.lower(), base.upper(), base.capitalize(), base.title()}
    if rules == "extended":
        variants.update(leet_variants(base))
    for v in variants:
        for suf in COMMON_SUFFIXES:
            yield v + suf
        if rules == "extended":
            for pre in COMMON_PREFIXES[1:]:
                yield pre + v


def hash_candidate(candidate, fmt):
    data = candidate.encode("utf-8", "ignore")
    if fmt == "ntlm":
        return hashlib.new("md4", candidate.encode("utf-16-le")).hexdigest()
    if fmt in RAW_FORMATS:
        return hashlib.new(fmt, data).hexdigest()
    # crypt(3): passing the full setting as salt reproduces the digest
    return crypt.crypt(candidate, fmt)


def check_chunk(args):
    words, target, fmt, rules = args
    for word in words:
        for cand in mutate(word, rules):
            try:
                digest = hash_candidate(cand, fmt)
            except (ValueError, OSError):
                continue
            if digest and digest.lower() == target.lower():
                return cand
    return None


def detect_format(target):
    if target.startswith("*") and len(target) == 41:
        return "mysql"
    for prefix, name in CRYPT_PREFIXES.items():
        if target.startswith(prefix):
            return name
    if len(target) == 32:
        return "md5"
    if len(target) == 40:
        return "sha1"
    if len(target) == 56:
        return "sha224"
    if len(target) == 64:
        return "sha256"
    if len(target) == 96:
        return "sha384"
    if len(target) == 128:
        return "sha512"
    return None


def chunks(items, size):
    for i in range(0, len(items), size):
        yield items[i:i + size]


def main():
    ap = argparse.ArgumentParser(description="offline dictionary hash cracker")
    ap.add_argument("--hash", required=True, help="target hash/digest")
    ap.add_argument("--format", help="md5|sha1|sha224|sha256|sha384|sha512|ntlm|sha512crypt|sha256crypt|md5crypt")
    ap.add_argument("--wordlist", required=True, help="wordlist file")
    ap.add_argument("--rules", choices=["none", "basic", "extended"], default="basic")
    ap.add_argument("--workers", type=int, default=max(1, (os.cpu_count() or 1) - 1))
    ap.add_argument("--timeout", type=float, default=0, help="abort after N seconds (0 = no limit)")
    args = ap.parse_args()

    target = args.hash.strip()
    fmt = (args.format or detect_format(target) or "").lower()
    if fmt == "mysql":
        print("[!] MySQL native hashes need the *-prefixed SHA1 chain; not supported", file=sys.stderr)
        return 2
    if fmt not in RAW_FORMATS and fmt not in CRYPT_PREFIXES.values():
        print("[!] could not determine format; pass --format", file=sys.stderr)
        return 2
    if fmt == "ntlm":
        try:
            hashlib.new("md4")
        except ValueError:
            print("[!] MD4 unavailable in this OpenSSL build; NTLM unsupported", file=sys.stderr)
            return 2
    if fmt in CRYPT_PREFIXES.values() and not HAS_CRYPT:
        print("[!] crypt(3) module unavailable on this platform", file=sys.stderr)
        return 2

    try:
        with open(args.wordlist, "r", errors="ignore") as fh:
            words = [w.rstrip("\n\r") for w in fh]
    except OSError as e:
        print(f"[!] cannot read wordlist: {e}", file=sys.stderr)
        return 2
    words = [w for w in words if w]
    if not words:
        print("[!] wordlist is empty", file=sys.stderr)
        return 2

    print(f"[*] format={fmt} rules={args.rules} words={len(words)} workers={args.workers}")
    start = time.time()
    work = list(chunks(words, 500))
    found = None

    with mp.Pool(args.workers) as pool:
        async_result = pool.imap_unordered(check_chunk, [(c, target, fmt, args.rules) for c in work])
        try:
            for result in async_result:
                if result:
                    found = result
                    pool.terminate()
                    break
                if args.timeout and (time.time() - start) > args.timeout:
                    print("[!] timeout reached", file=sys.stderr)
                    pool.terminate()
                    break
        except KeyboardInterrupt:
            pool.terminate()

    elapsed = time.time() - start
    if found:
        print(f"[+] CRACKED: {found!r}  ({elapsed:.1f}s)")
        return 0
    print(f"[-] not found in {len(words)} word(s) ({elapsed:.1f}s)")
    return 1


if __name__ == "__main__":
    sys.exit(main())
