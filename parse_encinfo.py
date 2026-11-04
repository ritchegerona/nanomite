#!/usr/bin/env python3
"""Parse the Excel file's EncryptionInfo stream to get real spin count and params."""
import olefile
import struct
import sys

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)

# Read EncryptionInfo stream
with ole.openstream("EncryptionInfo") as stream:
    data = stream.read()

print(f"EncryptionInfo stream size: {len(data)} bytes")

# Parse header (ECMA-376 Standard)
versionMajor, versionMinor = struct.unpack("<HH", data[:4])
print(f"Version: {versionMajor}.{versionMinor}")

if versionMajor in [2, 3, 4] and versionMinor == 2:  # Standard
    offset = 4
    flags = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    sizeExtra = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    algId = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    algIdHash = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    keySize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    providerType = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    reserved1 = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    reserved2 = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    
    # cspName is UTF-16LE string, rest of header
    cspName_end = data.find(b'\x00\x00', offset)
    cspName = data[offset:cspName_end].decode('utf-16le')
    offset = cspName_end + 2
    
    print(f"flags: {flags}")
    print(f"sizeExtra: {sizeExtra}")
    print(f"algId: 0x{algId:08X} ({'AES-128' if algId == 0x660E else 'AES-192' if algId == 0x660F else 'AES-256' if algId == 0x6610 else 'RC4'})")
    print(f"algIdHash: 0x{algIdHash:08X}")
    print(f"keySize: {keySize} bits")
    print(f"providerType: 0x{providerType:08X}")
    print(f"cspName: {cspName}")
    
    # Now parse verifier
    saltSize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    salt = data[offset:offset+saltSize]; offset += saltSize
    encryptedVerifier = data[offset:offset+16]; offset += 16
    verifierHashSize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    encryptedVerifierHash = data[offset:offset+verifierHashSize]; offset += verifierHashSize
    
    print(f"\nsaltSize: {saltSize}")
    print(f"salt: {salt.hex()}")
    print(f"encryptedVerifier: {encryptedVerifier.hex()}")
    print(f"verifierHashSize: {verifierHashSize}")
    print(f"encryptedVerifierHash: {encryptedVerifierHash.hex()}")

# Read EncryptedPackage to check for spin count in Agile header
# But this is Standard format, so spin count comes from the hash format
# The hash $office$*2007*20*... means spin count = 20

print("\n--- Hash format analysis ---")
print("$office$*2007*20*128*16*salt*verifier*verifierHash")
print("spin count from hash: 20")
print("John typically uses spinCount * 1000 = 20,000 iterations")
print("msoffcrypto hardcodes ITER_COUNT = 50,000")

# Try to read spin count from EncryptedPackage stream
try:
    with ole.openstream("EncryptedPackage") as stream:
        pkg_data = stream.read()
    print(f"\nEncryptedPackage stream size: {len(pkg_data)} bytes")
    # Standard format doesn't have spin count in package, it's in hash
except Exception as e:
    print(f"Could not read EncryptedPackage: {e}")

ole.close()