#!/usr/bin/env python3
import sys
sys.path.insert(0, '/opt/homebrew/lib/python3.15/site-packages')
from oletools.excel2john import Excel2John
import hashlib

filepath = '/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx'

# Try to extract hash
try:
    result = Excel2John(filepath).json_output
    print("Hash extracted:", result)
except Exception as e:
    print(f"Error with Excel2John: {e}")
    
# Alternative: try using msoffcrypto to get the hash
try:
    from msoffcrypto import OfficeFile
    with open(filepath, 'rb') as f:
        of = OfficeFile(f)
    print("Is password protected:", of.is_password_protected())
except Exception as e:
    print(f"Error with msoffcrypto: {e}")

# Try hydra if we have a hash
import subprocess
result = subprocess.run(['file', '--mime-encoding', filepath], capture_output=True, text=True)
print("File encoding:", result.stdout, result.stderr)