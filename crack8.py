import olefile
import io
from msoffcrypto.format.ooxml import _parseinfo
from msoffcrypto.method.rc4_cryptoapi import DocumentRC4CryptoAPI

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")

kind, info = _parseinfo(ei_stream)
print(f"Kind: {kind}")

salt = info["verifier"]["salt"]
keySize = info["header"]["keySize"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"]

print(f"salt: {salt.hex()}")
print(f"keySize: {keySize}")
print(f"encryptedVerifier: {encryptedVerifier.hex()}")
print(f"encryptedVerifierHash: {encryptedVerifierHash.hex()}")
print(f"encryptedVerifierHash length: {len(encryptedVerifierHash)}")

passwords = [
    "", "password", "Password", "PASSWORD", "123456", "12345", "123456789",
    "qwerty", "abc123", "letmein", "welcome", "monkey", "dragon", "master",
    "login", "admin", "admin123", "root", "test", "guest",
    "Safeway", "safeway", "SAFEWAY", "Safeway1", "safeway1", "Safeway123",
    "Medical", "medical", "MEDICAL", "Medical1", "medical1", "Medical123",
    "Staffing", "staffing", "STAFFING", "Staffing1", "staffing1", "Staffing123",
    "MSR", "msr", "MSR2026", "msr2026",
    "2026", "2026!", "1-9-2026", "01092026", "192026", "1/9/2026",
    "january2026", "January2026", "Jan2026", "jan2026",
    "ritche", "ritchegerona", "gerona",
    "Safeway2026", "safeway2026", "Medical2026", "medical2026",
    "Staffing2026", "staffing2026",
]

found = None
for pwd in passwords:
    if DocumentRC4CryptoAPI.verifypw(
        pwd, salt, keySize, encryptedVerifier, encryptedVerifierHash
    ):
        found = pwd
        print(f"FOUND: '{pwd}'")
        break
    else:
        print(f"FAIL: '{pwd}'")

if not found:
    print("NOT FOUND")
    exit(1)
