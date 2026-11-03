#!/usr/bin/env python3
"""Generate contextual password candidates from file metadata."""

words = [
    "Safeway", "safeway", "SAFEWAY",
    "Medical", "medical", "MEDICAL",
    "Staffing", "staffing", "STAFFING",
    "MSR", "msr", "Msr",
    "2026", "2025", "2024", "2023",
]

separators = ["", " ", "_", "-", "@", "!", "#", "$", "%", "&"]

dates = [
    "1-9-2026", "1-9-26", "01092026", "01/09/2026", "2026-01-09",
    "09012026", "09/01/2026", "Jan2026", "jan2026", "JAN2026",
    "January2026", "january2026",
]

# Build combinations
candidates = set()

# Single words
for w in words:
    candidates.add(w)

# Word + number
for w in words:
    for n in ["1", "123", "1234", "12345", "123456", "2026", "2025", "2024"]:
        candidates.add(w + n)
        candidates.add(n + w)

# Word + separator + number
for w in words:
    for s in separators:
        for n in ["2026", "123", "1234", "12345"]:
            candidates.add(w + s + n)
            candidates.add(n + s + w)

# Two words
for w1 in words:
    for w2 in words:
        if w1.lower() != w2.lower():
            candidates.add(w1 + w2)
            candidates.add(w1 + "_" + w2)
            candidates.add(w1 + " " + w2)

# Word + date
for w in words:
    for d in dates:
        candidates.add(w + d)
        candidates.add(d + w)

# Common patterns with year
for p in ["Summer", "summer", "Winter", "winter", "Spring", "spring", "Fall", "fall"]:
    candidates.add(p + "2026")
    candidates.add(p + "2025")

# Common business passwords
common = [
    "Welcome2026", "welcome2026", "Welcome2025", "welcome2025",
    "Admin2026", "admin2026", "Admin2025", "admin2025",
    "Password2026", "password2026", "Password2025", "password2025",
    "Test2026", "test2026", "Test2025", "test2025",
    "Hello2026", "hello2026", "Hello2025", "hello2025",
    "Safeway2026!", "safeway2026!", "Medical2026!", "medical2026!",
    "Staffing2026!", "staffing2026!", "MSR2026!", "msr2026!",
    "Safeway@2026", "safeway@2026", "Medical@2026", "medical@2026",
    "Staffing@2026", "staffing@2026", "MSR@2026", "msr@2026",
    "Safeway#2026", "safeway#2026", "Medical#2026", "medical#2026",
    "Safeway$2026", "safeway$2026", "Medical$2026", "medical$2026",
]

for c in common:
    candidates.add(c)

# Names that might be relevant
names = [
    "Ritche", "ritche", "RitcheGerona", "ritchegerona",
    "Gerona", "gerona",
]
for n in names:
    candidates.add(n)
    candidates.add(n + "2026")
    candidates.add(n + "123")

with open("context_wordlist.txt", "w") as f:
    for c in sorted(candidates):
        f.write(c + "\n")

print(f"Generated {len(candidates)} candidates")