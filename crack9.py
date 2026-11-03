import olefile
from msoffcrypto.format.ooxml import _parseinfo
from msoffcrypto.method.ecma376_standard import ECMA376Standard

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")

kind, info = _parseinfo(ei_stream)
print(f"Kind: {kind}")

salt = info["verifier"]["salt"]
algId = info["header"]["algId"]
algIdHash = info["header"]["algIdHash"]
keySize = info["header"]["keySize"]
providerType = info["header"]["providerType"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"]

print(f"salt: {salt.hex()}")
print(f"algId: {algId:#x}")
print(f"algIdHash: {algIdHash:#x}")
print(f"keySize: {keySize}")
print(f"providerType: {providerType}")

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
    key = ECMA376Standard.makekey_from_password(
        pwd, algId, algIdHash, providerType, keySize,
        16, salt
    )
    if ECMA376Standard.verifykey(key, encryptedVerifier, encryptedVerifierHash):
        found = pwd
        print(f"FOUND: '{pwd}'")
        break
    else:
        print(f"FAIL: '{pwd}'")

if not found:
    print("NOT FOUND")
    exit(1)
