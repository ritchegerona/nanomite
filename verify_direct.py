import olefile
import io
from struct import unpack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.decrepit.ciphers.algorithms import ARC4
from cryptography.hazmat.primitives.ciphers import Cipher

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei = ole.openstream("EncryptionInfo")
data = ei.read()

# Office 2007 Standard encryption format
versionMajor, versionMinor = unpack("<HH", data[0:4])
print(f"Version: {versionMajor}.{versionMinor}")

encryptionHeaderSize = unpack("<I", data[4:8])[0]
print(f"Header size: {encryptionHeaderSize}")

header_blob = io.BytesIO(data[8:8+encryptionHeaderSize])
flags = unpack("<I", header_blob.read(4))[0]
sizeExtra = unpack("<I", header_blob.read(4))[0]
algId = unpack("<I", header_blob.read(4))[0]
algIdHash = unpack("<I", header_blob.read(4))[0]
keySize = unpack("<I", header_blob.read(4))[0]
providerType = unpack("<I", header_blob.read(4))[0]
reserved1 = unpack("<I", header_blob.read(4))[0]
reserved2 = unpack("<I", header_blob.read(4))[0]
cspName = header_blob.read().decode("utf-16le", errors="replace")

print(f"algId: {algId:#x}")
print(f"algIdHash: {algIdHash:#x}")
print(f"keySize: {keySize}")
print(f"CSP: {cspName}")

if keySize == 0:
    keySize = 40

verifier_offset = 8 + encryptionHeaderSize
verifier_blob = io.BytesIO(data[verifier_offset:])
saltSize = unpack("<I", verifier_blob.read(4))[0]
salt = verifier_blob.read(16)
encryptedVerifier = verifier_blob.read(16)
verifierHashSize = unpack("<I", verifier_blob.read(4))[0]
encryptedVerifierHash = verifier_blob.read(verifierHashSize)

print(f"saltSize: {saltSize}")
print(f"salt: {salt.hex()}")
print(f"encryptedVerifier: {encryptedVerifier.hex()}")
print(f"encryptedVerifierHash: {encryptedVerifierHash.hex()}")

def verify_password(password):
    password_bytes = password.encode("UTF-16LE")
    h0 = sha1(salt + password_bytes).digest()
    blockbytes = pack("<I", 0)
    hfinal = sha1(h0 + blockbytes).digest()
    if keySize == 40:
        key = hfinal[:5] + b"\x00" * 11
    else:
        key = hfinal[: keySize // 8]
    
    cipher = Cipher(ARC4(key), mode=None, backend=default_backend())
    decryptor = cipher.decryptor()
    verifier = decryptor.update(encryptedVerifier)
    verifierHash = decryptor.update(encryptedVerifierHash)
    computed_hash = sha1(verifier).digest()
    return computed_hash == verifierHash

print(f"\nTest 'password': {verify_password('password')}")
print(f"Test '123456': {verify_password('123456')}")
