import msoffcrypto, io, sys
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
    # Try all 4-digit PINs
    candidates = [f"{i:04d}" for i in range(10000)]
    print(f"Trying {len(candidates)} 4-digit PINs with {cpu_count()} workers...")

    found = None
    with Pool(cpu_count()) as pool:
        for i, result in enumerate(pool.imap_unordered(try_pw, candidates, chunksize=50)):
            if result:
                found = result
                pool.terminate()
                print(f"\n*** FOUND: {repr(result)} ***")
                break
            if (i + 1) % 2000 == 0:
                print(f"  tried {i + 1}/{len(candidates)}...", flush=True)

    if not found:
        print("\nNo 4-digit PIN match.")
