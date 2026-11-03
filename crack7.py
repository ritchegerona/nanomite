import olefile
import io
from msoffcrypto.format.common import _parse_header_RC4CryptoAPI
from msoffcrypto.method.rc4_cryptoapi import DocumentRC4CryptoAPI

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei = ole.openstream("EncryptionInfo")
data = ei.read()

info = _parse_header_RC4CryptoAPI(io.BytesIO(data))

print("salt:", info["salt"].hex())
print("keySize:", info["keySize"])
print("encryptedVerifier:", info["encryptedVerifier"].hex())
print("encryptedVerifierHash:", info["encryptedVerifierHash"].hex())

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
        pwd, info["salt"], info["keySize"],
        info["encryptedVerifier"], info["encryptedVerifierHash"]
    ):
        found = pwd
        print(f"FOUND: '{pwd}'")
        break
    else:
        print(f"FAIL: '{pwd}'")

if not found:
    print("NOT FOUND")
    exit(1)
