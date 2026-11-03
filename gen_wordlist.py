#!/usr/bin/env python3
"""Generate a comprehensive wordlist for Excel password cracking."""
words = []

# Base words
base = [
    "", "password", "Password", "PASSWORD", "pass", "Pass", "PASS",
    "123456", "12345", "123456789", "12345678", "1234567", "1234567890",
    "qwerty", "Qwerty", "QWERTY", "abc123", "ABC123",
    "letmein", "Letmein", "welcome", "Welcome", "monkey", "Monkey",
    "dragon", "Dragon", "master", "Master", "login", "Login",
    "admin", "Admin", "ADMIN", "admin123", "Admin123",
    "root", "Root", "test", "Test", "guest", "Guest",
    "Safeway", "safeway", "SAFEWAY", "Medical", "medical", "MEDICAL",
    "Staffing", "staffing", "STAFFING", "MSR", "msr", "MSR",
    "HR", "hr", "Hr", "Employee", "employee", "Nurse", "nurse",
    "Doctor", "doctor", "Clinic", "clinic", "Hospital", "hospital",
]

# Add combinations
for b in base:
    words.append(b)
    words.append(b + "1")
    words.append(b + "123")
    words.append(b + "1234")
    words.append(b + "12345")
    words.append(b + "2026")
    words.append(b + "2025")
    words.append(b + "!")
    words.append(b + "@")
    words.append(b + "#")
    words.append("1" + b)
    words.append("123" + b)

# Date patterns from filename 1-9-2026
dates = [
    "2026", "2026!", "2026@", "2026#", "2026123", "1232026",
    "01092026", "01/09/2026", "1-9-2026", "1/9/2026", "192026",
    "010926", "09012026", "09-01-2026", "09/01/2026",
    "jan9", "Jan9", "JAN9", "january", "January", "JANUARY",
    "jan2026", "Jan2026", "JAN2026", "january2026", "January2026",
]
words.extend(dates)

# Company + year combos
combos = [
    "Safeway2026", "safeway2026", "SAFEWAY2026",
    "Safeway2026!", "safeway2026!", "Safeway2026@",
    "Safeway20261", "Safeway2026123",
    "Medical2026", "medical2026", "MEDICAL2026",
    "Staffing2026", "staffing2026", "STAFFING2026",
    "MSR2026", "msr2026", "MSR2026!",
    "Safeway Medical", "safeway medical",
    "Medical Staffing", "medical staffing",
    "Safeway Staffing", "safeway staffing",
    "Safeway123", "safeway123", "Safeway1234",
    "Medical123", "medical123", "Medical1234",
    "Staffing123", "staffing123", "Staffing1234",
]
words.extend(combos)

# Names
names = ["ritche", "Ritche", "RITCHE", "ritchegerona", "gerona", "Gerona", "GERONA"]
for n in names:
    words.append(n)
    words.append(n + "1")
    words.append(n + "123")
    words.append(n + "2026")

# Simple patterns
simple = [
    "aaaaaa", "aaaaaa1", "111111", "11111111", "000000", "00000000",
    "123123", "123123123", "654321", "6543210",
    "abc123", "abc1234", "abc12345",
    "qwerty123", "qwerty1234", "qwerty12345",
]
words.extend(simple)

# Deduplicate
seen = set()
unique = []
for w in words:
    if w not in seen:
        seen.add(w)
        unique.append(w)

with open("/tmp/custom_wordlist.txt", "w") as f:
    for w in unique:
        f.write(w + "\n")

print(f"Generated {len(unique)} unique passwords")
