import msoffcrypto
import io
import sys
from msoffcrypto import exceptions

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
wordlist = "/tmp/wordlist.txt"

with open(filepath, "rb") as f:
    file_bytes = f.read()

found = False
count = 0
with open(wordlist, "r") as wl:
    for line in wl:
        pwd = line.strip()
        if not pwd:
            continue
        count += 1
        try:
            test = msoffcrypto.OfficeFile(io.BytesIO(file_bytes))
            test.load_key(password=pwd, verify_password=True)
            print(f"FOUND: '{pwd}'")
            found = True
            break
        except exceptions.InvalidKeyError:
            pass
        except Exception as e:
            print(f"ERROR on '{pwd}': {e}")
        if count % 1000 == 0:
            print(f"tried {count}...", flush=True)

if not found:
    print(f"NOT FOUND after {count} passwords")
    sys.exit(1)
