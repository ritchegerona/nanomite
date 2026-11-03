#!/usr/bin/env python3
"""Search encrypted Excel file for embedded password hints in ASCII and UTF-16LE."""

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

with open(filepath, 'rb') as f:
    data = f.read()

print(f"File size: {len(data)} bytes")
print(f"First 16 bytes: {data[:16].hex()}")

print("\n--- ASCII strings (length >= 6) ---")
i = 0
ascii_strings = []
while i < len(data):
    if 32 <= data[i] < 127:
        start = i
        while i < len(data) and 32 <= data[i] < 127:
            i += 1
        s = data[start:i].decode('ascii')
        if len(s) >= 6:
            ascii_strings.append((start, s))
    else:
        i += 1

for offset, s in ascii_strings:
    print(f"  0x{offset:06X}: {s}")

print(f"\nTotal ASCII strings found: {len(ascii_strings)}")

print("\n--- UTF-16LE strings (length >= 4 chars) ---")
i = 0
utf16_strings = []
while i < len(data) - 2:
    if data[i+1] == 0 and 32 <= data[i] < 127:
        start = i
        while i < len(data) - 1 and data[i+1] == 0 and 32 <= data[i] < 127:
            i += 2
        try:
            s = data[start:i].decode('utf-16-le')
        except:
            i += 2
            continue
        if len(s) >= 4:
            utf16_strings.append((start, s))
    else:
        i += 2

for offset, s in utf16_strings:
    print(f"  0x{offset:06X}: {s}")

print(f"\nTotal UTF-16LE strings found: {len(utf16_strings)}")

# Search specifically for password-related keywords
print("\n--- Searching for password-related keywords ---")
keywords = [b'password', b'Password', b'PASSWORD', b'pass', b'Pass', b'PASS',
            b'key', b'Key', b'KEY', b'secret', b'Secret', b'SECRET',
            b'hint', b'Hint', b'HINT']
for kw in keywords:
    idx = 0
    while True:
        idx = data.find(kw, idx)
        if idx == -1:
            break
        # Show context
        context = data[max(0,idx-20):idx+len(kw)+20]
        printable = ''.join(chr(b) if 32<=b<127 else '.' for b in context)
        print(f"  0x{idx:06X}: ...{printable}...")
        idx += 1