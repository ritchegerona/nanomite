#!/usr/bin/env python3
"""Quick crack using msoffcrypto's own verification on high-probability passwords."""
import olefile
from struct import unpack
from msoffcrypto.format.ooxml import _parseinfo
from msoffcrypto.method.ecma376_standard import ECMA376Standard
import msoffcrypto, io, sys

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

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

def verify_password(pwd):
    key = ECMA376Standard.makekey_from_password(
        pwd, algId, algIdHash, providerType, keySize, 16, salt
    )
    return ECMA376Standard.verifykey(key, encryptedVerifier, encryptedVerifierHash)

# High-probability passwords
passwords = [
    # Most common
    "", "password", "Password", "PASSWORD", "123456", "12345", "123456789",
    "qwerty", "abc123", "letmein", "welcome", "monkey", "dragon", "master",
    "login", "admin", "admin123", "root", "test", "guest",
    # Company/context
    "Safeway", "safeway", "SAFEWAY", "Safeway1", "safeway1",
    "Medical", "medical", "MEDICAL", "Medical1", "medical1",
    "Staffing", "staffing", "STAFFING", "Staffing1", "staffing1",
    "MSR", "msr", "MSR2026", "msr2026",
    # Dates from filename
    "2026", "2026!", "1-9-2026", "01092026", "192026", "1/9/2026",
    "january2026", "January2026", "Jan2026", "jan2026",
    # Names
    "ritche", "ritchegerona", "gerona",
    # Combos
    "Safeway2026", "safeway2026", "Medical2026", "medical2026",
    "Staffing2026", "staffing2026",
    "Safeway123", "safeway123", "Medical123", "medical123",
    "Staffing123", "staffing123",
    # 4-digit common
    "0000", "1111", "1234", "4321", "9999", "1004", "2000",
    "2020", "2024", "2025",
    # 6-digit common
    "000000", "123456", "654321", "111111", "999999",
]

for pwd in passwords:
    try:
        if verify_password(pwd):
            print(f"\n*** FOUND: '{pwd}' ***")
            # Decrypt
            with open(filepath, "rb") as f:
                of = msoffcrypto.OfficeFile(f)
            of.load_key(password=pwd)
            out = io.BytesIO()
            of.decrypt(out)
            output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
            with open(output_path, "wb") as f:
                f.write(out.getvalue())
            print(f"Decrypted file written to: {output_path}")
            print(f"Size: {len(out.getvalue())} bytes")
            sys.exit(0)
        else:
            print(f"FAIL: '{pwd}'")
    except Exception as e:
        print(f"ERROR: '{pwd}': {e}")

print("\nNOT FOUND")
sys.exit(1)
