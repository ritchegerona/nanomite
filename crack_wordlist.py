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

words = []
with open('/tmp/wordlist.txt') as f:
    for line in f:
        line = line.strip()
        if line:
            words.append(line)

print(f"Trying {len(words)} passwords...")

found = None
for i, pw in enumerate(words):
    if try_decrypt(pw):
        found = pw
        print(f"\n*** FOUND: {repr(pw)} ***")
        break
    if (i + 1) % 1000 == 0:
        print(f"  tried {i + 1}/{len(words)}...", flush=True)

if not found:
    print("\nNo match found.")
