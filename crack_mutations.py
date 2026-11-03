#!/usr/bin/env python3
"""Try wordlist passwords with common mutations."""
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
    with open(filepath, "rb") as f:
        of = msoffcrypto.OfficeFile(f)
    of.load_key(password=password)
    out = io.BytesIO()
    of.decrypt(out)
    return out.getvalue()

# Base passwords from wordlist + common mutations
base_passwords = []
with open("/tmp/wordlist.txt") as wl:
    for line in wl:
        pwd = line.strip()
        if pwd:
            base_passwords.append(pwd)

print(f"Loaded {len(base_passwords)} base passwords")

# Generate mutations
mutations = []
for bp in base_passwords:
    mutations.append(bp)
    mutations.append(bp + "1")
    mutations.append(bp + "123")
    mutations.append(bp + "2026")
    mutations.append(bp + "!")
    mutations.append(bp + "1234")
    mutations.append("1" + bp)
    mutations.append("123" + bp)

# Deduplicate while preserving order
seen = set()
candidates = []
for m in mutations:
    if m not in seen:
        seen.add(m)
        candidates.append(m)

print(f"Trying {len(candidates)} mutated candidates...")

found = None
count = 0
for pwd in candidates:
    count += 1
    try:
        if verify_password_fast(pwd):
            found = pwd
            print(f"\n*** FOUND: '{pwd}' ***")
            break
    except Exception:
        pass
    if count % 500 == 0:
        print(f"  tried {count}/{len(candidates)}...", flush=True)

if found:
    data = decrypt_file(found)
    output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
    with open(output_path, "wb") as f:
        f.write(data)
    print(f"\nDecrypted file written to: {output_path}")
    print(f"Size: {len(data)} bytes")
    sys.exit(0)

print(f"\nNOT FOUND after {count} candidates.")
sys.exit(1)
