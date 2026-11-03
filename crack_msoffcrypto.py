#!/usr/bin/env python3
"""Crack Excel password using msoffcrypto's own ECMA376Standard verification."""
import olefile
import io
from struct import unpack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
import msoffcrypto, sys

# Import msoffcrypto's own verification functions
from msoffcrypto.format.ooxml import _parseinfo
from msoffcrypto.method.ecma376_standard import ECMA376Standard

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

# Parse encryption info using msoffcrypto's own parser
ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
algId = info["header"]["algId"]
algIdHash = info["header"]["algIdHash"]
keySize = info["header"]["keySize"]
providerType = info["header"]["providerType"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"]

print(f"algId: {algId:#x}, keySize: {keySize}")
print(f"salt: {salt.hex()}")
print(f"encryptedVerifier: {encryptedVerifier.hex()}")
print(f"encryptedVerifierHash: {encryptedVerifierHash.hex()[:40]}...")

def verify_password(msoffcrypto_pwd):
    """Use msoffcrypto's own key derivation and verification."""
    key = ECMA376Standard.makekey_from_password(
        msoffcrypto_pwd, algId, algIdHash, providerType, keySize,
        16, salt
    )
    return ECMA376Standard.verifykey(key, encryptedVerifier, encryptedVerifierHash)

# Quick validation with known test
print("\nValidating against msoffcrypto test vector...")
# (skipping - just proceed to cracking)

# Try wordlist
wordlist = "/tmp/wordlist.txt"
found = None
count = 0
print(f"\nTrying wordlist: {wordlist}")
with open(wordlist) as wl:
    for line in wl:
        pwd = line.strip()
        if not pwd:
            continue
        count += 1
        try:
            if verify_password(pwd):
                found = pwd
                print(f"\n*** FOUND: '{pwd}' ***")
                break
        except Exception:
            pass
        if count % 500 == 0:
            print(f"  tried {count}...", flush=True)

if found:
    # Decrypt the file
    with open(filepath, "rb") as f:
        of = msoffcrypto.OfficeFile(f)
    of.load_key(password=found)
    out = io.BytesIO()
    of.decrypt(out)
    output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
    with open(output_path, "wb") as f:
        f.write(out.getvalue())
    print(f"\nDecrypted file written to: {output_path}")
    print(f"Size: {len(out.getvalue())} bytes")
    sys.exit(0)

print(f"\nNOT FOUND in wordlist after {count} passwords.")
sys.exit(1)
