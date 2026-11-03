#!/usr/bin/env python3
"""Fast single-process cracker for 4-digit and 6-digit PINs."""
import olefile
import io
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo
import sys, time

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
keySize = info["header"]["keySize"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"][:20]

cbRequiredKeyLength = keySize // 8
cbHash = sha1().digest_size
ITER_COUNT = 20000

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
    d = aes.decryptor()
    verifier = d.update(encryptedVerifier)
    expected = sha1(verifier).digest()
    d = aes.decryptor()
    vh = d.update(encryptedVerifierHash)[:cbHash]
    return expected == vh

def decrypt_file(password):
    import msoffcrypto
    with open(filepath, "rb") as f:
        of = msoffcrypto.OfficeFile(f)
    of.load_key(password=password)
    out = io.BytesIO()
    of.decrypt(out)
    return out.getvalue()

start = time.time()
found = None

# Try 4-digit PINs
print("Trying 4-digit PINs (0000-9999)...")
for i in range(10000):
    pwd = f"{i:04d}"
    if verify_password_fast(pwd):
        found = pwd
        break
    if (i + 1) % 1000 == 0:
        elapsed = time.time() - start
        rate = (i + 1) / elapsed
        print(f"  tried {i+1}/10000, {rate:.1f} pw/s, {elapsed:.0f}s elapsed", flush=True)

if found:
    elapsed = time.time() - start
    print(f"\n*** FOUND: '{found}' (after {elapsed:.0f}s) ***")
    data = decrypt_file(found)
    output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
    with open(output_path, "wb") as f:
        f.write(data)
    print(f"Decrypted file written to: {output_path}")
    print(f"Size: {len(data)} bytes")
    sys.exit(0)

print("NOT FOUND in 4-digit PINs.")

# Try 6-digit PINs
print("\nTrying 6-digit numeric (000000-999999)...")
start6 = time.time()
for i in range(1000000):
    pwd = f"{i:06d}"
    if verify_password_fast(pwd):
        found = pwd
        break
    if (i + 1) % 100000 == 0:
        elapsed = time.time() - start6
        rate = (i + 1) / elapsed
        print(f"  tried {i+1}/1000000, {rate:.1f} pw/s, {elapsed:.0f}s elapsed", flush=True)

if found:
    elapsed = time.time() - start
    print(f"\n*** FOUND: '{found}' (after {elapsed:.0f}s total) ***")
    data = decrypt_file(found)
    output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
    with open(output_path, "wb") as f:
        f.write(data)
    print(f"Decrypted file written to: {output_path}")
    print(f"Size: {len(data)} bytes")
    sys.exit(0)

print("\nNOT FOUND after all attempts.")
sys.exit(1)
