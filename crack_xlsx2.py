import msoffcrypto, io

IN_PATH = '/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx'

with open(IN_PATH, 'rb') as f:
    FILE_DATA = f.read()

def try_decrypt(pw):
    try:
        of = msoffcrypto.OfficeFile(io.BytesIO(FILE_DATA))
        of.load_key(password=pw)
        out = io.BytesIO()
        of.decrypt(out)
        out.seek(0)
        return out.read(4) == b'PK\x03\x04'
    except Exception:
        return False

candidates = []

# Date variations from filename 1-9-2026
candidates.extend([
    '192026', '0192026', '12092026', '01092026',
    '202619', '2026019', '192026', '260109', '260901',
    '01-09-26', '01-09-2026', '1-9-26', '1-9-2026',
    '01/09/26', '01/09/2026', '1/9/26', '1/9/2026',
    'Jan92026', 'Jan926', '9Jan2026', '9Jan26',
    'January92026', 'January926',
    '09012026', '090126',
    '2026-01-09', '2026-09-01',
    '20260109', '20260901',
])

# Common business/corporate passwords
base_words = [
    'safeway', 'Safeway', 'SAFEWAY',
    'medical', 'Medical', 'MEDICAL',
    'staffing', 'Staffing', 'STAFFING',
    'msr', 'MSR', 'Msr',
    'password', 'Password', 'PASSWORD',
    'admin', 'Admin', 'ADMIN',
    'welcome', 'Welcome', 'WELCOME',
    'company', 'Company', 'COMPANY',
    'corporate', 'Corporate', 'CORPORATE',
    'employee', 'Employee', 'EMPLOYEE',
    'health', 'Health', 'HEALTH',
    'nurse', 'Nurse', 'NURSE',
    'doctor', 'Doctor', 'DOCTOR',
    'hospital', 'Hospital', 'HOSPITAL',
    'clinic', 'Clinic', 'CLINIC',
    'care', 'Care', 'CARE',
]

suffixes = ['', '123', '1234', '12345', '1', '2', '2023', '2024', '2025', '2026', '!', '@', '#', '2023!', '2024!', '2025!', '2026!']

for w in base_words:
    for s in suffixes:
        candidates.append(w + s)
        candidates.append(s + w)

# Common patterns
candidates.extend([
    'Password1', 'Password123', 'Password1234', 'Password2023', 'Password2024', 'Password2025', 'Password2026',
    'Welcome1', 'Welcome123', 'Welcome2023', 'Welcome2024', 'Welcome2025', 'Welcome2026',
    'Admin123', 'Admin2023', 'Admin2024', 'Admin2025', 'Admin2026',
    'Qwerty123', 'Qwerty2023', 'Qwerty2024', 'Qwerty2025', 'Qwerty2026',
    'Summer2023', 'Summer2024', 'Summer2025', 'Summer2026',
    'Winter2023', 'Winter2024', 'Winter2025', 'Winter2026',
    'Spring2023', 'Spring2024', 'Spring2025', 'Spring2026',
    'Fall2023', 'Fall2024', 'Fall2025', 'Fall2026',
    'January2026', 'February2026', 'March2026', 'April2026',
    'May2026', 'June2026', 'July2026', 'August2026',
    'September2026', 'October2026', 'November2026', 'December2026',
    '1-9-2026', '01-09-2026', '01092026', '12092026',
    'Safeway2026', 'Medical2026', 'Staffing2026', 'MSR2026',
    'Safeway123', 'Medical123', 'Staffing123', 'MSR123',
    'Safeway!', 'Medical!', 'Staffing!', 'MSR!',
])

print(f"Trying {len(candidates)} candidates...")

found = None
for i, pw in enumerate(candidates):
    if try_decrypt(pw):
        found = pw
        print(f"\n*** FOUND: {repr(pw)} ***")
        break
    if (i + 1) % 500 == 0:
        print(f"  tried {i + 1}/{len(candidates)}...", flush=True)

if not found:
    print("\nNo match found.")
