#!/usr/bin/env python3
"""Targeted password attempts for Excel file."""
import olefile
import io
from struct import unpack, pack
from hashlib import sha1
from cryptography.hazmat.backends import default_backend
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from msoffcrypto.format.ooxml import _parseinfo
import msoffcrypto, sys

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
keySize = info["header"]["keySize"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"][:20]

cbRequiredKeyLength = keySize // 8
cbHash = sha1().digest_size
ITER_COUNT = 20000

buf1_36 = bytearray(b"\x36" * 64)
buf2_5c = bytearray(b"\x5c" * 64)

def verify_password_fast(password):
    password_bytes = password.encode("UTF-16LE")
    h = sha1(salt + password_bytes).digest()
    for i in range(ITER_COUNT):
        h = sha1(pack("<I", i) + h).digest()
    hfinal = sha1(h + pack("<I", 0)).digest()
    buf1 = bytes(a ^ b for a, b in zip(hfinal[:cbHash], buf1_36[:cbHash])) + bytes(buf1_36[cbHash:])
    x1 = sha1(buf1).digest()
    buf2 = bytes(a ^ b for a, b in zip(hfinal[:cbHash], buf2_5c[:cbHash])) + bytes(buf2_5c[cbHash:])
    x2 = sha1(buf2).digest()
    keyDerived = (x1 + x2)[:cbRequiredKeyLength]
    aes = Cipher(algorithms.AES(keyDerived), modes.ECB(), backend=default_backend())
    d = aes.decryptor()
    verifier = d.update(encryptedVerifier)
    expected = sha1(verifier).digest()
    d = aes.decryptor()
    vh = d.update(encryptedVerifierHash)[:cbHash]
    return expected == vh

def decrypt_file(password):
    with open(filepath, "rb") as f:
        of = msoffcrypto.OfficeFile(f)
    of.load_key(password=password)
    out = io.BytesIO()
    of.decrypt(out)
    return out.getvalue()

passwords = [
    # Date variations from filename 1-9-2026
    "01092026", "01/09/2026", "1-9-2026", "1/9/2026", "192026", "1-9-26", "1/9/26",
    "010926", "jan92026", "Jan92026", "January92026", "January-2026", "january-2026",
    "2026-01-09", "20260109", "09012026", "09-01-2026", "09/01/2026",
    # Year + context
    "Safeway2026!", "safeway2026!", "Medical2026!", "medical2026!",
    "Staffing2026!", "staffing2026!", "MSR2026!", "msr2026!",
    "Safeway2026#", "safeway2026#", "Medical2026#", "Staffing2026#",
    # Company + number
    "Safeway1234", "safeway1234", "Medical1234", "medical1234",
    "Staffing1234", "staffing1234", "MSR1234", "msr1234",
    "Safeway12345", "Medical12345", "Staffing12345",
    # Common password patterns
    "Password1", "Password123", "Password1234", "Password12345",
    "Welcome1", "Welcome123", "Welcome1234",
    "Admin1234", "Admin12345", "Administrator", "admin12345",
    "Qwerty123", "Qwerty1234", "Qwerty12345",
    "Dragon123", "Dragon1234", "Monkey123", "Monkey1234",
    "Summer2026", "Winter2026", "Spring2026", "Fall2026",
    "Summer2025", "Winter2025", "Spring2025", "Fall2025",
    # Employee/HR context
    "HR2026", "hr2026", "HR1234", "hr1234", "Employee", "employee",
    "Staff2026", "staff2026", "Staff1234", "staff1234",
    "Nurse2026", "nurse2026", "Doctor2026", "doctor2026",
    # Simple patterns
    "aaaaaa", "aaaaaa1", "11111111", "123123123", "1234567890",
    "password123", "password1234", "password12345",
    "Safeway!", "safeway!", "Medical!", "Staffing!", "MSR!",
    "Safeway@2026", "safeway@2026", "Medical@2026", "Staffing@2026",
    # Name variations
    "Ritche", "Ritche1", "Ritche123", "Ritche1234", "Gerona1", "Gerona123",
    # Blank variations
    "null", "NULL", "None", "NONE",
]

found = None
for pwd in passwords:
    try:
        if verify_password_fast(pwd):
            found = pwd
            print(f"\n*** FOUND: '{pwd}' ***")
            break
        print(f"FAIL: '{pwd}'")
    except Exception as e:
        print(f"ERROR: '{pwd}': {e}")

if found:
    data = decrypt_file(found)
    output_path = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"
    with open(output_path, "wb") as f:
        f.write(data)
    print(f"\nDecrypted file written to: {output_path}")
    print(f"Size: {len(data)} bytes")
    sys.exit(0)

print("\nNOT FOUND in targeted list 2.")
sys.exit(1)
