import msoffcrypto
import io
import sys

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

passwords = [
    # Company/context
    "Safeway", "safeway", "SAFEWAY", "Safeway1", "safeway1",
    "Medical", "medical", "MEDICAL", "Medical1", "medical1",
    "Staffing", "staffing", "STAFFING", "Staffing1", "staffing1",
    "Medical Staffing", "medical staffing",
    "Safeway Medical", "safeway medical",
    "Safeway Staffing", "safeway staffing",
    # Dates from filename
    "1-9-2026", "192026", "01092026", "1926", "19-26", "1/9/2026",
    "Jan92026", "jan92026", "January92026", "january2026",
    "2026", "2026!", "2026Safeway", "2026Medical",
    # Common business/HR passwords
    "admin", "password", "Password", "PASSWORD",
    "123456", "12345", "123456789", "qwerty", "abc123",
    "letmein", "welcome", "monkey", "dragon", "master",
    "login", "admin123", "root", "test", "guest",
    # Names
    "ritche", "ritchegerona", "gerona",
    # Variations
    "Safeway2026", "safeway2026", "Medical2026", "medical2026",
    "Staffing2026", "staffing2026",
    "Safeway123", "safeway123", "Medical123", "medical123",
    "Staffing123", "staffing123",
    "MSR", "msr", "MSR2026", "msr2026",
    "SAFEWAY2026", "MEDICAL2026", "STAFFING2026",
    # Blank-ish
    "", " ", "  ",
]

found = False
for pwd in passwords:
    try:
        test = msoffcrypto.OfficeFile(open(filepath, "rb"))
        test.load_key(password=pwd)
        decrypted = io.BytesIO()
        test.decrypt(decrypted)
        print(f"FOUND: '{pwd}'")
        found = True
        break
    except Exception as e:
        print(f"FAIL: '{pwd}'")

if not found:
    print("NOT FOUND in targeted list")
    sys.exit(1)
