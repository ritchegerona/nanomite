import msoffcrypto, io
from multiprocessing import Pool, cpu_count

IN_PATH = '/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx'

with open(IN_PATH, 'rb') as f:
    FILE_DATA = f.read()

def try_pw(pw):
    try:
        of = msoffcrypto.OfficeFile(io.BytesIO(FILE_DATA))
        of.load_key(password=pw)
        out = io.BytesIO()
        of.decrypt(out)
        out.seek(0)
        if out.read(4) == b'PK\x03\x04':
            return pw
    except Exception:
        pass
    return None

if __name__ == '__main__':
    # Try 5-digit and 6-digit numeric passwords
    candidates = [f"{i:05d}" for i in range(100000)] + [f"{i:06d}" for i in range(1000000)]
    print(f"Trying {len(candidates)} numeric passwords (5-6 digits) with {cpu_count()} workers...")

    found = None
    count = 0
    with Pool(cpu_count()) as pool:
        for result in pool.imap_unordered(try_pw, candidates, chunksize=100):
            count += 1
            if result:
                found = result
                pool.terminate()
                print(f"\n*** FOUND: {repr(result)} ***")
                break
            if count % 50000 == 0:
                print(f"  tried {count}/{len(candidates)}...", flush=True)

    if not found:
        print(f"\nNo numeric match found (tried {count}).")
