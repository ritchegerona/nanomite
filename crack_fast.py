#!/usr/bin/env python3
"""Fast Excel password cracker using direct AES-ECB verification.
Correct iteration count: spin_count * 1000 = 20 * 1000 = 20000."""
import olefile
import io
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo
from concurrent.futures import ProcessPoolExecutor, as_completed
import sys

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

# Extract encryption parameters once
ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
algId = info["header"]["algId"]
algIdHash = info["header"]["algIdHash"]
keySize = info["header"]["keySize"]
providerType = info["header"]["providerType"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"][:20]

cbRequiredKeyLength = keySize // 8
cbHash = sha1().digest_size
ITER_COUNT = 20000  # spin_count (20) * 1000

buf1_36 = bytearray(b"\x36" * 64)
buf2_5c = bytearray(b"\x5c" * 64)

# Pre-create AES cipher objects won't work since key changes per password.
# But we can precompute the XOR masks.

def verify_password_fast(password):
    password_bytes = password.encode("UTF-16LE")
    h = sha1(salt + password_bytes).digest()
    for i in range(ITER_COUNT):
        h = sha1(pack("<I", i) + h).digest()

    blockbytes = pack("<I", 0)
    hfinal = sha1(h + blockbytes).digest()

    buf1 = bytes(a ^ b for a, b in zip(hfinal[:cbHash], buf1_36[:cbHash])) + bytes(buf1_36[cbHash:])
    x1 = sha1(buf1).digest()

    buf2 = bytes(a ^ b for a, b in zip(hfinal[:cbHash], buf2_5c[:cbHash])) + bytes(buf2_5c[cbHash:])
    x2 = sha1(buf2).digest()

    keyDerived = (x1 + x2)[:cbRequiredKeyLength]

    aes = Cipher(algorithms.AES(keyDerived), modes.ECB(), backend=default_backend())
    decryptor = aes.decryptor()
    verifier = decryptor.update(encryptedVerifier)
    expected_hash = sha1(verifier).digest()

    decryptor = aes.decryptor()
    verifierHash = decryptor.update(encryptedVerifierHash)[:cbHash]

    return expected_hash == verifierHash

# Try targeted passwords first
targeted = [
    "", " ", "password", "Password", "PASSWORD", "123456", "12345", "123456789",
    "qwerty", "abc123", "letmein", "welcome", "monkey", "dragon", "master",
    "login", "admin", "admin123", "root", "test", "guest",
    "Safeway", "safeway", "SAFEWAY", "Safeway1", "safeway1", "Safeway123",
    "Medical", "medical", "MEDICAL", "Medical1", "medical1", "Medical123",
    "Staffing", "staffing", "STAFFING", "Staffing1", "staffing1", "Staffing123",
    "MSR", "msr", "MSR2026", "msr2026",
    "2026", "2026!", "1-9-2026", "01092026", "192026", "1/9/2026",
    "january2026", "January2026", "Jan2026", "jan2026",
    "ritche", "ritchegerona", "gerona",
    "Safeway2026", "safeway2026", "Medical2026", "medical2026",
    "Staffing2026", "staffing2026",
    "Safeway Medical", "safeway medical",
    "Safeway Staffing", "safeway staffing",
    "Medical Staffing", "medical staffing",
    "safeway2026!", "Safeway2026!", "Medical2026!", "Staffing2026!",
    "Safeway1!", "safeway1!", "Medical1!", "Staffing1!",
    "Safeway123!", "safeway123!", "Medical123!", "Staffing123!",
]

print(f"Testing {len(targeted)} targeted passwords...")
for pwd in targeted:
    try:
        if verify_password_fast(pwd):
            print(f"\n*** FOUND: '{pwd}' ***")
            sys.exit(0)
    except Exception:
        pass
print("Not found in targeted list.")

# Try 4-digit and 6-digit PINs
print("\nTrying 4-digit PINs (0000-9999)...")
found = None
with ProcessPoolExecutor() as ex:
    futures = {ex.submit(verify_password_fast, f"{i:04d}"): f"{i:04d}" for i in range(10000)}
    for fut in as_completed(futures):
        if fut.result():
            found = futures[fut]
            print(f"\n*** FOUND: '{found}' ***")
            ex.shutdown(wait=False)
            sys.exit(0)

print("Not found in 4-digit PINs.")

# Try 6-digit numeric (common for PINs)
print("\nTrying 6-digit numeric (000000-999999) - this may take a while...")
count = 0
for i in range(1000000):
    pwd = f"{i:06d}"
    count += 1
    try:
        if verify_password_fast(pwd):
            print(f"\n*** FOUND: '{pwd}' ***")
            sys.exit(0)
    except Exception:
        pass
    if count % 100000 == 0:
        print(f"  tried {count}/1000000...", flush=True)

print("\nNOT FOUND after all attempts.")
sys.exit(1)
