#!/usr/bin/env python3
"""Generate more password variations."""
words = set()

# Base words
base = ["Safeway", "safeway", "SAFEWAY", "Medical", "medical", "MEDICAL",
        "Staffing", "staffing", "STAFFING", "MSR", "msr", "MSR",
        "password", "Password", "PASSWORD", "admin", "Admin",
        "HR", "hr", "Hr", "employee", "Employee"]

# Numbers to append/prepend
nums = ["", "1", "12", "123", "1234", "12345", "123456",
        "01", "001", "0001", "2026", "2025", "2024",
        "2026!", "2026@", "2026#", "2026$",
        "1!", "123!", "1234!", "12345!"]

# Special chars
specials = ["", "!", "@", "#", "$", "%", "&", "*", "1!", "123!", "1234!"]

for b in base:
    for n in nums:
        for s in specials:
            words.add(b + n + s)
            words.add(n + b + s)
            words.add(b + s + n)
            words.add(n + s + b)

# Date variations
dates = ["2026", "2026!", "2026@", "01092026", "01-09-2026", "01/09/2026",
         "1-9-2026", "1/9/2026", "192026", "010926", "09012026",
         "Jan9", "jan9", "JAN9", "January2026", "january2026"]
for d in dates:
    words.add(d)
    for b in base[:3]:
        words.add(b + d)
        words.add(d + b)

# Simple sequences
for i in range(10000):
    words.add(f"{i:04d}")
    words.add(f"{i:06d}")

# Common patterns
patterns = ["Welcome1", "Welcome123", "Welcome1234",
            "Admin1234", "Admin12345", "Administrator123",
            "Manager123", "Manager1234", "Manager2026",
            "Office123", "Office1234", "Office2026",
            "Company123", "Company2026", "Business123",
            "Secure123", "Secure1234", "Secure2026"]
words.update(patterns)

# Remove empty
words.discard("")

with open("/tmp/custom_wordlist2.txt", "w") as f:
    for w in sorted(words):
        f.write(w + "\n")

print(f"Generated {len(words)} unique passwords")
