import msoffcrypto
import io
import sys
from msoffcrypto import exceptions

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

with open(filepath, "rb") as f:
    file_bytes = f.read()

passwords = [
    # Most common
    "", "password", "Password", "PASSWORD", "123456", "12345", "123456789",
    "qwerty", "abc123", "letmein", "welcome", "monkey", "dragon", "master",
    "login", "admin", "admin123", "root", "test", "guest",
    # Context
    "Safeway", "safeway", "SAFEWAY", "Safeway1", "safeway1", "Safeway123",
    "Medical", "medical", "MEDICAL", "Medical1", "medical1", "Medical123",
    "Staffing", "staffing", "STAFFING", "Staffing1", "staffing1", "Staffing123",
    "MSR", "msr", "MSR2026", "msr2026",
    # Dates
    "2026", "2026!", "1-9-2026", "01092026", "192026", "1/9/2026",
    "january2026", "January2026", "Jan2026", "jan2026",
    # Names
    "ritche", "ritchegerona", "gerona",
    # Combos
    "Safeway2026", "safeway2026", "Medical2026", "medical2026",
    "Staffing2026", "staffing2026",
    "Safeway Medical", "safeway medical",
    "Safeway Staffing", "safeway staffing",
    "Medical Staffing", "medical staffing",
]

found = False
for pwd in passwords:
    try:
        test = msoffcrypto.OfficeFile(io.BytesIO(file_bytes))
        test.load_key(password=pwd, verify_password=True)
        print(f"FOUND: '{pwd}'")
        found = True
        break
    except exceptions.InvalidKeyError:
        print(f"FAIL: '{pwd}'")
    except Exception as e:
        print(f"ERROR: '{pwd}': {e}")

if not found:
    print("NOT FOUND")
    sys.exit(1)
