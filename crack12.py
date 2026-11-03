import olefile
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
wordlist = "/tmp/wordlist.txt"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
algId = info["header"]["algId"]
algIdHash = info["header"]["algIdHash"]
keySize = info["header"]["keySize"]
providerType = info["header"]["providerType"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"][:20]  # SHA-1 size

print(f"algId: {algId:#x}, algIdHash: {algIdHash:#x}, keySize: {keySize}")
print(f"salt: {salt.hex()}")
print(f"encryptedVerifier: {encryptedVerifier.hex()}")
print(f"encryptedVerifierHash: {encryptedVerifierHash.hex()}")

cbRequiredKeyLength = keySize // 8
cbHash = sha1().digest_size
ITER_COUNT = 50000

buf1_36 = bytearray(b"\x36" * 64)
buf2_5c = bytearray(b"\x5c" * 64)

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

print(f"Test 'password': {verify_password_fast('password')}")
print(f"Test '123456': {verify_password_fast('123456')}")

found = None
count = 0
with open(wordlist) as wl:
    for line in wl:
        pwd = line.strip()
        if not pwd:
            continue
        count += 1
        try:
            if verify_password_fast(pwd):
                found = pwd
                print(f"\nFOUND: '{pwd}'")
                break
        except Exception:
            pass
        if count % 500 == 0:
            print(f"tried {count}...", flush=True)

if not found:
    print(f"\nNOT FOUND after {count} passwords")
    exit(1)
