#!/usr/bin/env python3
"""Compare Python msoffcrypto vs C implementation."""
import olefile
from struct import unpack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo
from msoffcrypto.method.ecma376_standard import ECMA376Standard

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
algId = info["header"]["algId"]
algIdHash = info["header"]["algIdHash"]
keySize = info["header"]["keySize"]
providerType = info["header"]["providerType"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"]

def py_verify(pwd):
    key = ECMA376Standard.makekey_from_password(
        pwd, algId, algIdHash, providerType, keySize, 16, salt
    )
    return ECMA376Standard.verifykey(key, encryptedVerifier, encryptedVerifierHash)

# Test a few passwords with both Python (msoffcrypto) and report
for pwd in ["password", "123456", "admin", "test", "Safeway"]:
    result = py_verify(pwd)
    print(f"Python msoffcrypto '{pwd}': {result}")
