import olefile
import io
from struct import unpack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.decrepit.ciphers.algorithms import ARC4

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei = ole.openstream("EncryptionInfo")
data = ei.read()

# Parse RC4 CryptoAPI header per ms-offcrypto spec
blob = io.BytesIO(data)
versionMajor, versionMinor = unpack("<HH", blob.read(4))
flags = unpack("<I", blob.read(4))[0]
headerSize = unpack("<I", blob.read(4))[0]
sizeExtra = unpack("<I", blob.read(4))[0]
algId = unpack("<I", blob.read(4))[0]
algIdHash = unpack("<I", blob.read(4))[0]
keySize = unpack("<I", blob.read(4))[0]
providerType = unpack("<I", blob.read(4))[0]
reserved1 = unpack("<I", blob.read(4))[0]
reserved2 = unpack("<I", blob.read(4))[0]
cspName = blob.read().decode("utf-16le")

if keySize == 0:
    keySize = 40  # default 40-bit

# Now parse verifier from the rest of the data
# The verifier starts after the encryption header
verifier_blob = io.BytesIO(data[headerSize:]) if headerSize < len(data) else None

# Actually, the full data includes header + verifier. Let me re-parse from offset headerSize
blob2 = io.BytesIO(data[headerSize:])
saltSize = unpack("<I", blob2.read(4))[0]
salt = blob2.read(16)
encryptedVerifier = blob2.read(16)
verifierHashSize = unpack("<I", blob2.read(4))[0]
encryptedVerifierHash = blob2.read(verifierHashSize)

print(f"algId: {algId:#x} (AES-128)")
print(f"algIdHash: {algIdHash:#x}")
print(f"keySize: {keySize}")
print(f"salt: {salt.hex()}")
print(f"encryptedVerifier: {encryptedVerifier.hex()}")
print(f"encryptedVerifierHash: {encryptedVerifierHash.hex()}")

def verify_password(password):
    password_bytes = password.encode("UTF-16LE")
    h0 = sha1(salt + password_bytes).digest()
    blockbytes = unpack("<I", b"\x00\x00\x00\x00")[0]  # block=0
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

# Test with a known password to validate the algorithm
print("\nTesting algorithm with 'password':", verify_password("password"))
