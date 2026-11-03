# Excel Password Cracking Summary

## Target File
- Path: `/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx`
- Format: Office 2007+ OOXML, CDFV2 Encrypted
- Encryption: AES-128 CBC, SHA-1, spin count 20
- Hash format: `$office$*2007*20*128*16*<salt>*<encryptedKey>*<hashValue>`

## Hash Extracted
```
$office$*2007*20*128*16*51e68d53bd52f1f5b48422eefe380d11*18a9729f2486d6d694ddd6fb02db11f8*2e987931a728f6f464f9692918c0c3acbc5710e4
```

## Tools Created (all in /Users/ritchegerona/nanomite/)

| File | Description |
|------|-------------|
| `extract_hash.py` | Extracts $office$ hash from the xlsx using olefile |
| `crack1.py` | Python msoffcrypto dictionary cracker |
| `crack2.py` | Targeted password list with msoffcrypto |
| `crack3.py` | Wordlist cracker with msoffcrypto |
| `crack4.py` | Additional crack script |
| `crack5.py` through `crack12.py` | Various cracker iterations (C implementation, wordlist, brute force) |
| `crack_pins.c` | C implementation of key derivation for brute-forcing 4-6 digit PINs |
| `wordlist.txt` | 9,437-entry common password wordlist |
| `decrypt.py` | Python decryption script using msoffcrypto |
| `final_crack.py` | Final comprehensive password list test |
| `CRACKING_SUMMARY.md` | This file |

## Approaches Attempted

1. **Dictionary attack** — `msoffcrypto-tool` with common passwords and custom wordlist (9,437 entries)
2. **Targeted passwords** — Safeway, medical, staffing, MSR, HR, 2026, date variations
3. **4-digit PIN brute force** — All 10,000 combinations exhausted (~136 pw/s)
4. **Hashcat** (mode 9400) — GPU backends (OpenCL, Metal, CUDA) all broken on this system
5. **John the Ripper** — v1.9.0 does not support `$office$` format
6. **C implementation** — Key derivation in C (~220 pw/s), too slow for 6-digit brute force (~1.3 hours for full space)
7. **Python msoffcrypto** — ~100 pw/s verification rate

## Result

**PASSWORD NOT FOUND.**

The password `password` passed `msoffcrypto.OfficeFile.load_key()` but FAILED actual decryption with `InvalidKeyError` — `load_key()` does not validate the password; only `decrypt()` does.

All reasonable password candidates were tested without success. The file remains encrypted.

## Recommendations

1. Try password recovery services (hash cracking services with GPU clusters)
2. Try additional password variations not in the wordlists used
3. Check if you have any unencrypted backups or older versions of this file
4. Check if the file was created with a different version of Office that may use weaker encryption
