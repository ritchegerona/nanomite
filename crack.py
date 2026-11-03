import msoffcrypto
import sys
import io

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
wordlist = "/tmp/wordlist.txt"

with open(wordlist, "r") as wl:
    for i, line in enumerate(wl):
        pwd = line.strip()
        if not pwd:
            continue
        try:
            test = msoffcrypto.OfficeFile(open(filepath, "rb"))
            test.load_key(password=pwd)
            decrypted = io.BytesIO()
            test.decrypt(decrypted)
            print(f"FOUND: {pwd}")
            sys.exit(0)
        except Exception:
            pass
        if i % 500 == 0:
            print(f"tried {i}: {pwd}", flush=True)

print("NOT FOUND")
sys.exit(1)
