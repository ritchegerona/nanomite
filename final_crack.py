#!/usr/bin/env python3
"""Final comprehensive crack attempt using msoffcrypto."""
import msoffcrypto
import hashlib

FILE = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"

# Comprehensive password list
passwords = [
    # Common
    "password", "123456", "123456789", "qwerty", "abc123", "12345678", "111111",
    "1234567", "1234567890", "123123", "000000", "1234", "12345", "letmein",
    "monkey", "dragon", "master", "sunshine", "princess", "football", "baseball",
    "welcome", "shadow", "solo", "access", "michael", "superman", "batman",
    "trustno1", "login", "admin", "root", "test", "guest", "user",
    # Business/HR
    "safeway", "Safeway", "SAFEWAY", "medical", "Medical", "MEDICAL",
    "staffing", "Staffing", "STAFFING", "msr", "MSR", "safeway123",
    "Safeway123", "medicalstaffing", "MedicalStaffing", "staffing2026",
    "SafewayStaffing", "safewaystaffing", "hr", "HR", "humanresources",
    # Year/date based
    "2026", "2025", "2024", "192026", "19-2026", "1-9-2026", "01092026",
    "09012026", "192026", "jan92026", "Jan92026", "January2026",
    # Common patterns
    "Password", "Password1", "password1", "Password123", "password123",
    "123456!", "12345678!", "Qwerty123", "qwerty123", "Abc123!",
    "Welcome1", "Admin123", "Test123", "Guest123",
    # Simple words
    "hello", "world", "computer", "internet", "keyboard", "password1",
    "iloveyou", "sunshine", "monkey", "dragon", "master",
]

found = None
for pw in passwords:
    try:
        with open(FILE, 'rb') as f:
            of = msoffcrypto.OfficeFile(f)
            of.load_key(password=pw)
        found = pw
        print(f"FOUND: password = '{pw}'")
        break
    except Exception:
        pass

if not found:
    print("No match found in comprehensive list")
