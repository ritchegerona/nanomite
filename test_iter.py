#!/usr/bin/env python3
"""Test different iteration counts against known passwords."""
import olefile
import io
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo
import msoffcrypto, sys

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

buf1_36 = bytearray(b"\x36" * 64)
buf2_5c = bytearray(b"\x5c" * 64)

def verify_with_iter(password, iter_count):
    password_bytes = password.encode("UTF-16LE")
    h = sha1(salt + password_bytes).digest()
    for i in range(iter_count):
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

# Test common passwords with iter_count = 20
for pwd in ["password", "123456", "admin", "Safeway", "Medical", "Staffing"]:
    result = verify_with_iter(pwd, 20)
    print(f"iter=20, '{pwd}': {result}")

# Also try iter_count = 20000
print("\nWith iter=20000:")
for pwd in ["password", "123456"]:
    result = verify_with_iter(pwd, 20000)
    print(f"iter=20000, '{pwd}': {result}")
