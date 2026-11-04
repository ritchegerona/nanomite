#!/usr/bin/env python3
"""Verify password using msoffcrypto's own API with actual file data."""
import olefile
import struct
import sys

sys.path.insert(0, '/opt/homebrew/lib/python3.15/site-packages')

from msoffcrypto.method.ecma376_standard import ECMA376Standard

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)

# Read EncryptionInfo stream
with ole.openstream("EncryptionInfo") as stream:
    data = stream.read()

print(f"EncryptionInfo stream: {len(data)} bytes")
print(f"Hex (first 80 bytes): {data[:80].hex()}")

versionMajor, versionMinor = struct.unpack("<HH", data[:4])
print(f"Version: {versionMajor}.{versionMinor}")

offset = 4
flags = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
sizeExtra = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
algId = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
algIdHash = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
keySize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
providerType = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
reserved1 = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
reserved2 = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4

print(f"\nFixed header fields (32 bytes):")
print(f"  flags: {flags} (0x{flags:08X})")
print(f"  sizeExtra: {sizeExtra}")
print(f"  algId: 0x{algId:08X}")
print(f"  algIdHash: 0x{algIdHash:08X}")
print(f"  keySize: {keySize}")
print(f"  providerType: 0x{providerType:08X}")
print(f"  reserved1: {reserved1}")
print(f"  reserved2: {reserved2}")

# Skip sizeExtra bytes of extra data
print(f"\nSkipping {sizeExtra} bytes of extra data at offset {offset}")
offset += sizeExtra

# Read CSP Name (null-terminated UTF-16LE)
cspName = ""
while offset + 1 < len(data):
    ch = struct.unpack("<H", data[offset:offset+2])[0]
    if ch == 0:
        offset += 2
        break
    cspName += chr(ch)
    offset += 2
print(f"  cspName: '{cspName}'")
print(f"  Offset after cspName: {offset}")

# Parse EncryptionVerifier
saltSize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
salt = data[offset:offset+saltSize]; offset += saltSize
encryptedVerifier = data[offset:offset+16]; offset += 16
verifierHashSize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
encryptedVerifierHash = data[offset:offset+verifierHashSize]; offset += verifierHashSize

print(f"\nVerifier (at offset {offset - 4 - saltSize - 16 - 4 - verifierHashSize}):")
print(f"  saltSize: {saltSize}")
print(f"  salt: {salt.hex()}")
print(f"  encryptedVerifier: {encryptedVerifier.hex()}")
print(f"  verifierHashSize: {verifierHashSize}")
print(f"  encryptedVerifierHash: {encryptedVerifierHash.hex()}")

# The hash format says keySize=128 bits = AES-128
# But algId from file says 0x24 = RC4... Let's use what the hash says
# $office$*2007*20*128*16*... means 128-bit key

# The spin count from hash is 20. John uses spinCount * 1000 = 20000
# msoffcrypto hardcodes ITER_COUNT = 50000
# Let's test both

print("\n--- Testing passwords ---")
test_passwords = ["password", "123456", "1234", "0000", "admin", "safeway", "msr", "staffing", "medical", "12345", "12345678", "qwerty", "letmein"]

for pw in test_passwords:
    try:
        key = ECMA376Standard.makekey_from_password(
            pw, algId, algIdHash, providerType, keySize, saltSize, salt
        )
        valid = ECMA376Standard.verifykey(key, encryptedVerifier, encryptedVerifierHash)
        print(f"  '{pw}': {'VALID! PASSWORD FOUND!' if valid else 'invalid'}")
    except Exception as e:
        print(f"  '{pw}': ERROR {e}")

ole.close()