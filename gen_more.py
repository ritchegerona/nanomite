#!/usr/bin/env python3
"""Generate more targeted password candidates."""

candidates = set()

# 5 and 6 digit PINs - common patterns
for i in range(100000):
    candidates.add(f"{i:05d}")
for i in range(1000000):
    candidates.add(f"{i:06d}")

# Year-based patterns
for year in ["2026", "2025", "2024", "26", "25", "24"]:
    candidates.add(year)
    candidates.add(f"Safeway{year}")
    candidates.add(f"safeway{year}")
    candidates.add(f"Medical{year}")
    candidates.add(f"medical{year}")
    candidates.add(f"Staffing{year}")
    candidates.add(f"staffing{year}")
    candidates.add(f"MSR{year}")
    candidates.add(f"msr{year}")

# Month/year patterns
months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
          "January", "February", "March", "April", "May", "June", "July", "August",
          "September", "October", "November", "December"]
for m in months:
    candidates.add(f"{m}2026")
    candidates.add(f"{m}2025")
    candidates.add(f"{m}2024")
    candidates.add(f"{m.lower()}2026")
    candidates.add(f"{m.lower()}2025")

# Date patterns from filename
candidates.add("1-9-2026")
candidates.add("1-9-26")
candidates.add("01-09-2026")
candidates.add("010926")
candidates.add("090126")
candidates.add("192026")
candidates.add("912026")

# Common business/HR passwords
hr_passwords = [
    "HR2026", "hr2026", "HR2025", "hr2025",
    "Payroll2026", "payroll2026",
    "Employee2026", "employee2026",
    "Personnel2026", "personnel2026",
    "Confidential2026", "confidential2026",
    "Secure2026", "secure2026",
    "Protected2026", "protected2026",
    "Private2026", "private2026",
    "Restricted2026", "restricted2026",
    "Internal2026", "internal2026",
]
for p in hr_passwords:
    candidates.add(p)

# Keyboard patterns
kp = ["qwerty", "qwertyuiop", "asdfghjkl", "zxcvbnm",
      "qwerty123", "qwerty123456", "123qwerty",
      "asdf1234", "zxcv1234"]
for p in kp:
    candidates.add(p)
    candidates.add(p.upper())
    candidates.add(p.capitalize())

with open("more_wordlist.txt", "w") as f:
    for c in sorted(candidates):
        f.write(c + "\n")

print(f"Generated {len(candidates)} candidates")