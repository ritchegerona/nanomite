import msoffcrypto
from io import BytesIO
import time

filepath = '/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/MEDICAL STAFFING (MSR)_6-11-2026.xlsx'

with open(filepath, 'rb') as f:
    data = f.read()

bio = BytesIO(data)
office = msoffcrypto.OfficeFile(bio)

start = time.time()
found = False

# Brute force 4-digit PINs
for i in range(10000):
    pwd = f'{i:04d}'
    try:
        office.load_key(pwd, '')
        try:
            office.decrypt()
            print(f'SUCCESS with PIN: {pwd}')
            found = True
            break
        except Exception as e:
            if i < 10 or i % 2000 == 0:
                print(f'{i}: load_key OK, decrypt failed: {pwd}')
    except Exception as e:
        if i < 10:
            print(f'{i}: Failed load_key: {type(e).__name__}')

elapsed = time.time() - start
print(f'Time elapsed: {elapsed:.1f}s')
if not found:
    print('PIN not found in 0000-9999')
