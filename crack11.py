import olefile
import io
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.decrepit.ciphers.algorithms import ARC4
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
wordlist = "/tmp/wordlist.txt"

# Extract encryption parameters once
ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
data = ei_stream.read()

versionMajor, versionMinor = unpack("<HH", data[0:4])
encryptionHeaderSize = unpack("<I", data[4:8])[0]

header_blob = io.BytesIO(data[8:8+encryptionHeaderSize])
flags = unpack("<I", header_blob.read(4))[0]
sizeExtra = unpack("<I", header_blob.read(4))[0]
algId = unpack("<I", header_blob.read(4))[0]
algIdHash = unpack("<I", header_blob.read(4))[0]
keySize = unpack("<I", header_blob.read(4))[0]
providerType = unpack("<I", header_blob.read(4))[0]

verifier_offset = 8 + encryptionHeaderSize
verifier_blob = io.BytesIO(data[verifier_offset:])
saltSize = unpack("<I", verifier_blob.read(4))[0]
salt = verifier_blob.read(16)
encryptedVerifier = verifier_blob.read(16)
verifierHashSize = unpack("<I", verifier_blob.read(4))[0]
encryptedVerifierHash = verifier_blob.read(verifierHashSize)

print(f"algId: {algId:#x}, keySize: {keySize}")

# Precompute constants
cbRequiredKeyLength = keySize // 8
cbHash = 20  # SHA-1 digest size
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
    
    buf1 = bytearray(hfinal[:cbHash]) ^ buf1_36[:cbHash]
    buf1 += buf1_36[cbHash:]
    x1 = sha1(bytes(buf1)).digest()
    
    buf2 = bytearray(hfinal[:cbHash]) ^ buf2_5c[:cbHash]
    buf2 += buf2_5c[cbHash:]
    x2 = sha1(bytes(buf2)).digest()
    
    keyDerived = (x1 + x2)[:cbRequiredKeyLength]
    
    # AES-ECB verify
    aes = Cipher(algorithms.AES(keyDerived), modes.ECB(), backend=default_backend())
    decryptor = aes.decryptor()
    verifier = decryptor.update(encryptedVerifier)
    expected_hash = sha1(verifier).digest()
    decryptor = aes.decryptor()
    verifierHash = decryptor.update(encryptedVerifierHash)[:cbHash]
    
    return expected_hash == verifierHash

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
