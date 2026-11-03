#!/usr/bin/env python3
# office2john.py - Extract John the Ripper hash from Office files
# Based on JohnTheRipper's office2john.py

import sys
import os

try:
    from olefile import isOleFile, OleFileIO
except ImportError:
    print(f"{os.path.basename(sys.argv[0])}: olefile python module is missing", file=sys.stderr)
    sys.exit(1)

PY3 = sys.version_info[0] == 3
if PY3:
    from io import BytesIO as StringIO
else:
    from StringIO import StringIO
from struct import unpack
import binascii

def process_new_office(filename):
    try:
        ole = OleFileIO(filename)
    except Exception as e:
        sys.stderr.write(f"{filename}: cannot open as OLE file: {e}\n")
        return -1

    try:
        stream = ole.openstream("EncryptionInfo")
    except Exception:
        # Try EncryptionPackage
        try:
            stream = ole.openstream("EncryptionPackage")
        except Exception:
            sys.stderr.write(f"{filename}: no EncryptionInfo stream found\n")
            return -1

    major_version = unpack("<h", stream.read(2))[0]
    minor_version = unpack("<h", stream.read(2))[0]
    encryptionFlags = unpack("<I", stream.read(4))[0]

    if major_version == 0x04 and minor_version == 0x04:
        # Office 2010/2013+ (aggressive)
        sys.stderr.write(f"{filename}: Office 2010+ agile encryption not supported by this extractor\n")
        return -2
    else:
        # Office 2007 CryptoAPI
        stm = stream
        headerLength = unpack("<I", stm.read(4))[0]
        unpack("<I", stm.read(4))[0]  # skipFlags
        headerLength -= 4
        unpack("<I", stm.read(4))[0]  # sizeExtra
        headerLength -= 4
        unpack("<I", stm.read(4))[0]  # algId
        headerLength -= 4
        unpack("<I", stm.read(4))[0]  # algHashId
        headerLength -= 4
        keySize = unpack("<I", stm.read(4))[0]
        headerLength -= 4
        unpack("<I", stm.read(4))[0]  # providerType
        headerLength -= 4
        unpack("<I", stm.read(4))[0]  # unused
        headerLength -= 4
        unpack("<I", stm.read(4))[0]  # unused
        headerLength -= 4
        CSPName = stm.read(headerLength)
        # Encryption verifier
        saltSize = unpack("<I", stm.read(4))[0]
        salt = stm.read(saltSize)
        encryptedVerifier = stm.read(16)
        verifierHashSize = unpack("<I", stm.read(4))[0]
        encryptedVerifierHash = stm.read(verifierHashSize)

        sys.stdout.write(f"{os.path.basename(filename)}:$office$*2007*{verifierHashSize}*{keySize}*{saltSize}*{binascii.hexlify(salt).decode('ascii')}*{binascii.hexlify(encryptedVerifier).decode('ascii')}*{binascii.hexlify(encryptedVerifierHash)[0:64].decode('ascii')}\n")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <office-file>")
        sys.exit(1)
    process_new_office(sys.argv[1])
