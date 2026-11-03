#!/usr/bin/env python3
"""Single-process fast Excel password cracker with correct iteration count."""
import olefile
import io
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo
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

def verify_password_fast(password):
    password_bytes = password.encode("UTF-16LE")
    h = sha1(salt + password_bytes).digest()
    for i in range(ITER_COUNT):
        h = sha1(pack("<I", i) + h).digest()

    hfinal = sha1(h + pack("<I", 0)).digest()

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

# Try wordlist first
wordlist = "/tmp/wordlist.txt"
found = None
count = 0
print(f"Trying wordlist: {wordlist}")
with open(wordlist) as wl:
    for line in wl:
        pwd = line.strip()
        if not pwd:
            continue
        count += 1
        try:
            if verify_password_fast(pwd):
                found = pwd
                print(f"\n*** FOUND: '{pwd}' ***")
                break
        except Exception:
            pass
        if count % 1000 == 0:
            print(f"  tried {count}...", flush=True)

if found:
    # Decrypt the file
    import msoffcrypto
    with open(filepath, "rb") as f:
        of = msoffcrypto.OfficeFile(f)
    of.load_key(password=found)
    out = io.BytesIO()
    of.decrypt(out)
    out.seek(0)
    header = out.read(4)
    out.seek(0)
    output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
    with open(output_path, "wb") as f:
        f.write(out.getvalue())
    print(f"\nDecrypted file written to: {output_path}")
    print(f"Size: {len(out.getvalue())} bytes")
    print(f"Valid ZIP (Excel): {header == b'PK\\x03\\x04'}")
    sys.exit(0)

print(f"\nNOT FOUND in wordlist after {count} passwords.")

# Try 4-digit PINs
print("\nTrying 4-digit PINs (0000-9999)...")
for i in range(10000):
    pwd = f"{i:04d}"
    try:
        if verify_password_fast(pwd):
            found = pwd
            print(f"\n*** FOUND: '{pwd}' ***")
            break
    except Exception:
        pass
    if (i + 1) % 2000 == 0:
        print(f"  tried {i + 1}/10000...", flush=True)

if found:
    import msoffcrypto
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

print("NOT FOUND in 4-digit PINs.")

# Try 6-digit PINs
print("\nTrying 6-digit numeric (000000-999999)...")
for i in range(1000000):
    pwd = f"{i:06d}"
    try:
        if verify_password_fast(pwd):
            found = pwd
            print(f"\n*** FOUND: '{pwd}' ***")
            break
    except Exception:
        pass
    if (i + 1) % 100000 == 0:
        print(f"  tried {i + 1}/1000000...", flush=True)

if found:
    import msoffcrypto
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

print("\nNOT FOUND after all attempts.")
sys.exit(1)
