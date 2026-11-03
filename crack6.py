import msoffcrypto
import io
import sys
from msoffcrypto import exceptions
from concurrent.futures import ProcessPoolExecutor, as_completed

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

with open(filepath, "rb") as f:
    file_bytes = f.read()

def try_pwd(args):
    pwd, data = args
    try:
        test = msoffcrypto.OfficeFile(io.BytesIO(data))
        test.load_key(password=pwd, verify_password=True)
        return pwd
    except exceptions.InvalidKeyError:
        return None
    except Exception:
        return None

# Try 4-digit numeric passwords (0000-9999)
candidates = [f"{i:04d}" for i in range(10000)]

found = None
with ProcessPoolExecutor() as ex:
    futures = {ex.submit(try_pwd, (p, file_bytes)): p for p in candidates}
    for fut in as_completed(futures):
        r = fut.result()
        if r:
            found = r
            print(f"FOUND: '{r}'")
            break

if not found:
    print("NOT FOUND in 4-digit")
    sys.exit(1)
