#!/usr/bin/env python3
"""Generate common 6-digit PINs - dates, patterns, etc."""

candidates = set()

# MMDDYY dates for recent years
for year in ["24", "25", "26"]:
    for month in range(1, 13):
        for day in range(1, 29):
            candidates.add(f"{month:02d}{day:02d}{year}")
            candidates.add(f"{day:02d}{month:02d}{year}")

# DDMMYY dates
for year in ["24", "25", "26"]:
    for month in range(1, 13):
        for day in range(1, 29):
            candidates.add(f"{day:02d}{month:02d}{year}")

# YYMMDD dates
for year in ["24", "25", "26"]:
    for month in range(1, 13):
        for day in range(1, 29):
            candidates.add(f"{year}{month:02d}{day:02d}")

# Common repeating patterns
for d in range(10):
    candidates.add(str(d) * 6)

# Sequential
for start in range(10):
    s = "".join(str((start + i) % 10) for i in range(6))
    candidates.add(s)
    s = "".join(str((start - i) % 10) for i in range(6))
    candidates.add(s)

# Phone-like patterns
candidates.add("123456")
candidates.add("654321")
candidates.add("111111")
candidates.add("000000")
candidates.add("123123")
candidates.add("456456")
candidates.add("789789")
candidates.add("112233")
candidates.add("121212")

# Birth years + 2 digits
for year in ["24", "25", "26"]:
    for suffix in ["01", "02", "03", "04", "05", "10", "11", "12", "20", "21", "22", "23", "24", "25", "26", "50", "99"]:
        candidates.add(year + suffix)
        candidates.add(suffix + year)

# Common 6 digit passwords
common6 = [
    "123456", "654321", "111111", "000000", "123123", "121212",
    "112233", "789789", "456456", "100200", "200400",
    "654321", "135791", "246824",
    "101010", "202020", "303030",
    "111222", "222333", "333444",
    "123321", "456654", "789987",
    "202601", "202602", "202603",
    "010126", "010226", "010926",
    "090126", "010925", "010924",
]
for c in common6:
    candidates.add(c)

with open("six_digit_common.txt", "w") as f:
    for c in sorted(candidates):
        f.write(c + "\n")

print(f"Generated {len(candidates)} candidates")