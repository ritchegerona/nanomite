#!/usr/bin/env python3
"""Extract encryption info using msoffcrypto's own API."""
import olefile
import struct
import sys
import zipfile

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)

# Check if it's OLE or ZIP
print(f"Is OLE file: {olefile.isOleFile(open(filepath, 'rb'))}")
print(f"Is ZIP file: {zipfile.is_zipfile(filepath)}")

# Read EncryptionInfo stream
with ole.openstream("EncryptionInfo") as stream:
    data = stream.read()

print(f"\nEncryptionInfo stream: {len(data)} bytes")
print(f"Hex dump (first 200 bytes): {data[:200].hex()}")

# Parse version
versionMajor, versionMinor = struct.unpack("<HH", data[:4])
print(f"Version: {versionMajor}.{versionMinor}")

# For Standard encryption (version 2.x, 3.x, 4.x minor 2)
if versionMinor == 2:
    offset = 4
    flags = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    sizeExtra = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    algId = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    algIdHash = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    keySize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    providerType = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    reserved1 = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    reserved2 = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    
    # cspName is null-terminated UTF-16LE
    cspName = ""
    while offset + 1 < len(data):
        ch = struct.unpack("<H", data[offset:offset+2])[0]
        if ch == 0:
            offset += 2
            break
        cspName += chr(ch)
        offset += 2
    
    print(f"\nStandard Encryption Header:")
    print(f"  flags: {flags} (0x{flags:08X})")
    print(f"  sizeExtra: {sizeExtra}")
    print(f"  algId: 0x{algId:08X}")
    print(f"  algIdHash: 0x{algIdHash:08X}")
    print(f"  keySize: {keySize}")
    print(f"  providerType: 0x{providerType:08X}")
    print(f"  cspName: {cspName}")
    
    # Parse verifier
    saltSize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    salt = data[offset:offset+saltSize]; offset += saltSize
    encryptedVerifier = data[offset:offset+16]; offset += 16
    verifierHashSize = struct.unpack("<I", data[offset:offset+4])[0]; offset += 4
    encryptedVerifierHash = data[offset:offset+verifierHashSize]; offset += verifierHashSize
    
    print(f"\nVerifier:")
    print(f"  saltSize: {saltSize}")
    print(f"  salt: {salt.hex()}")
    print(f"  encryptedVerifier: {encryptedVerifier.hex()}")
    print(f"  verifierHashSize: {verifierHashSize}")
    print(f"  encryptedVerifierHash: {encryptedVerifierHash.hex()}")

# Also check if this might be Agile encryption
# Agile has version 4.4
if versionMajor == 4 and versionMinor == 4:
    print("\nThis is Agile encryption - need XML parsing")

ole.close()