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

candidates = [
    # Filename-based
    'MEDICAL STAFFING (MSR)_1-9-2026',
    'MEDICAL STAFFING (MSR)',
    'MEDICAL STAFFING',
    'MEDICAL_STAFFING',
    'medical_staffing_msr',
    'medical_staffing_msr_1_9_2026',
    'MSR_1-9-2026',
    'MSR192026',
    # Very common weak passwords
    '12345678910', '12345678911', '12345678912',
    'abcdefgh', 'abcdefghij', 'abcdefghi',
    'qazwsx', 'qazwsxedc', 'qazwsxedcrfv',
    '1q2w3e4r', '1q2w3e4r5t', '1qaz2wsx',
    '1qaz2wsx3edc', '1qaz2wsx3edc4rfv',
    '!@#$%^&*', '!@#$%^&*()',
    'P@ssw0rd', 'P@ssw0rd123', 'P@ssw0rd1',
    'Pass1234', 'Pass12345', 'Pass123456',
    'Hello123', 'Hello1234', 'Hello12345',
    'Computer123', 'Computer1234',
    'Summer123', 'Summer1234', 'Summer2023', 'Summer2024', 'Summer2025', 'Summer2026',
    'Winter123', 'Winter2023', 'Winter2024', 'Winter2025', 'Winter2026',
    'Spring123', 'Spring2023', 'Spring2024', 'Spring2025', 'Spring2026',
    'Fall123', 'Fall2023', 'Fall2024', 'Fall2025', 'Fall2026',
    'Christmas123', 'Christmas2023', 'Christmas2024', 'Christmas2025',
    'NewYear2024', 'NewYear2025', 'NewYear2026',
    # Simple words with caps
    'Monkey123', 'Dragon123', 'Master123', 'Shadow123',
    'Batman123', 'Superman123',
    # Repeated patterns
    '121212', '131313', '141414', '151515', '161616', '171717', '181818', '191919',
    '212121', '222222', '232323', '242424', '252525', '262626', '272727', '282828', '292929',
    'ababab', 'cdcdcd', 'efefef',
    # Common corporate
    'corporate123', 'corporate2023', 'corporate2024', 'corporate2025', 'corporate2026',
    'business123', 'business2023', 'business2024', 'business2025', 'business2026',
    'office123', 'office2023', 'office2024', 'office2025', 'office2026',
    'company123', 'company2023', 'company2024', 'company2025', 'company2026',
    # Safeway specific
    'safeway2023!', 'safeway2024!', 'safeway2025!', 'safeway2026!',
    'Safeway2023!', 'Safeway2024!', 'Safeway2025!', 'Safeway2026!',
    'SAFEWAY2023', 'SAFEWAY2024', 'SAFEWAY2025', 'SAFEWAY2026',
    # Medical staffing specific
    'medicalstaffing123', 'MedicalStaffing123',
    'medicalstaffing2023', 'MedicalStaffing2023',
    'medicalstaffing2024', 'MedicalStaffing2024',
    'medicalstaffing2025', 'MedicalStaffing2025',
    'medicalstaffing2026', 'MedicalStaffing2026',
    # Short common
    'aa123456', 'aa12345678', 'aa123456789',
    'a1b2c3', 'a1b2c3d4', 'a1b2c3d4e5',
]

print(f"Trying {len(candidates)} candidates...")

found = None
for i, pw in enumerate(candidates):
    if try_decrypt(pw):
        found = pw
        print(f"\n*** FOUND: {repr(pw)} ***")
        break
    if (i + 1) % 100 == 0:
        print(f"  tried {i + 1}/{len(candidates)}...", flush=True)

if not found:
    print("\nNo match found.")
