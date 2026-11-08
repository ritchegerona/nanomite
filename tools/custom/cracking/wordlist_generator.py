#!/usr/bin/env python3
"""Targeted wordlist generator with case, leet and suffix mutations.

Builds a de-duplicated candidate list from base words, optionally bounded by
length. Intended for authorized password audits and offline cracking runs.

Usage:
  python3 wordlist_generator.py --base acme,company2026 -o words.txt
  python3 wordlist_generator.py --base-file orgs.txt --rules extended --max-len 16
"""

import argparse
import itertools
import sys

LEET = {"a": ["a", "4", "@"], "e": ["e", "3"], "i": ["i", "1", "!"],
        "o": ["o", "0"], "s": ["s", "5", "$"], "t": ["t", "7"]}
DIGITS = ["", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
          "12", "123", "1234", "12345", "007"]
YEARS = ["", "2020", "2021", "2022", "2023", "2024", "2025", "2026"]
SYMBOLS = ["", "!", "!!", "@", "#", "$", "!@#", "1!", "123!", "01"]
SEPARATORS = ["", "_", "-", "."]


def leet_variants(word):
    out = [word]
    for i, ch in enumerate(word.lower()):
        if ch in LEET:
            for sub in LEET[ch][1:]:
                out.append(word[:i] + sub + word[i + 1:])
    return out


def variants(base, rules):
    base = base.strip()
    if not base:
        return
    yield base
    if rules == "none":
        return
    core = {base, base.lower(), base.upper(), base.capitalize(), base.title()}
    if rules == "extended":
        core.update(leet_variants(base))
    for c in core:
        yield c
        for d in DIGITS:
            yield c + d
        for y in YEARS:
            yield c + y
        if rules == "extended":
            for s in SYMBOLS:
                yield c + s
            for sep, d in itertools.product(SEPARATORS, DIGITS):
                if sep and d:
                    yield c + sep + d
            for d in DIGITS:
                yield d + c


def main():
    ap = argparse.ArgumentParser(description="targeted wordlist generator")
    ap.add_argument("--base", help="comma-separated base words")
    ap.add_argument("--base-file", help="file with one base word per line")
    ap.add_argument("--rules", choices=["none", "basic", "extended"], default="basic")
    ap.add_argument("--min-len", type=int, default=0)
    ap.add_argument("--max-len", type=int, default=64)
    ap.add_argument("--limit", type=int, default=0, help="stop after N unique entries (0 = all)")
    ap.add_argument("-o", "--output", help="output file (default: stdout)")
    args = ap.parse_args()

    bases = []
    if args.base:
        bases.extend(w.strip() for w in args.base.split(",") if w.strip())
    if args.base_file:
        with open(args.base_file) as fh:
            bases.extend(line.strip() for line in fh if line.strip() and not line.startswith("#"))
    if not bases:
        ap.error("provide --base or --base-file")

    seen = set()
    ordered = []
    for base in bases:
        for cand in variants(base, args.rules):
            if len(cand) < args.min_len or len(cand) > args.max_len:
                continue
            if cand in seen:
                continue
            seen.add(cand)
            ordered.append(cand)
            if args.limit and len(ordered) >= args.limit:
                break
        if args.limit and len(ordered) >= args.limit:
            break

    out = "\n".join(ordered) + "\n"
    if args.output:
        with open(args.output, "w") as fh:
            fh.write(out)
        print(f"[*] wrote {len(ordered)} entries to {args.output}", file=sys.stderr)
    else:
        sys.stdout.write(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
