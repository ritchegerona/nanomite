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
    # Keyboard patterns
    'qwerty', 'qwerty123', 'qwertyuiop', 'asdfgh', 'asdf1234', 'zxcvbn', 'zxcv1234',
    'qwertyui', 'asdfghjk', 'zxcvbnm',
    # Simple words
    'monkey', 'dragon', 'master', 'shadow', 'sunshine', 'trustno1', 'batman',
    'football', 'baseball', 'superman', 'michael', 'jordan', 'thomas', 'charlie',
    'andrew', 'joshua', 'matthew', 'daniel', 'christopher', 'james', 'robert',
    # Common with numbers
    'test123', 'test1234', 'demo123', 'demo1234', 'sample123', 'sample1234',
    'user123', 'user1234', 'guest123', 'guest1234',
    'letmein123', 'letmein1234', 'welcome123', 'welcome1234',
    'admin1234', 'admin12345', 'root1234', 'root12345',
    # Safeway/medical variations
    'safeway1', 'safeway2', 'safeway2023', 'safeway2024', 'safeway2025',
    'safeway2026', 'safeway@', 'safeway#', 'safeway!',
    'medical1', 'medical2', 'medical2023', 'medical2024', 'medical2025', 'medical2026',
    'staffing1', 'staffing2', 'staffing2023', 'staffing2024', 'staffing2025', 'staffing2026',
    'msr1', 'msr2', 'msr2023', 'msr2024', 'msr2025', 'msr2026',
    # Year variations
    '2026!', '2026@', '2026#', '2026$', '2026%', '2026^', '2026&',
    '!2026', '@2026', '#2026',
    '2025!', '2024!', '2023!', '2022!', '2021!', '2020!',
    # Simple phrases
    'iloveyou', 'princess', 'football1', 'baseball1', 'sunshine1',
    'password!', 'password@', 'password#', 'password12345',
    # All digits common
    '000000', '111111', '222222', '333333', '444444', '555555', '666666',
    '777777', '888888', '999999', '123321', '654321', '112233', '1234567',
    '123456789', '1234567890', '00000000', '11111111', '12341234',
    # Common short
    'abcd1234', 'abcd12345', 'xyz1234', 'test12345', 'demo12345',
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
