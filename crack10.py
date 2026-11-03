import olefile
from msoffcrypto.format.ooxml import _parseinfo
from msoffcrypto.method.ecma376_standard import ECMA376Standard

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
wordlist = "/tmp/wordlist.txt"

ole = olefile.OleFileIO(filepath)
ei_stream = ole.openstream("EncryptionInfo")
kind, info = _parseinfo(ei_stream)

salt = info["verifier"]["salt"]
algId = info["header"]["algId"]
algIdHash = info["header"]["algIdHash"]
keySize = info["header"]["keySize"]
providerType = info["header"]["providerType"]
encryptedVerifier = info["verifier"]["encryptedVerifier"]
encryptedVerifierHash = info["verifier"]["encryptedVerifierHash"]

print(f"algId: {algId:#x}, keySize: {keySize}")
print(f"Verifying {sum(1 for _ in open(wordlist))} passwords...")

found = None
count = 0
with open(wordlist) as wl:
    for line in wl:
        pwd = line.strip()
        if not pwd:
            continue
        count += 1
        try:
            key = ECMA376Standard.makekey_from_password(
                pwd, algId, algIdHash, providerType, keySize, 16, salt
            )
            if ECMA376Standard.verifykey(key, encryptedVerifier, encryptedVerifierHash):
                found = pwd
                print(f"\nFOUND: '{pwd}'")
                break
        except Exception:
            pass
        if count % 500 == 0:
            print(f"tried {count}...", flush=True)

if not found:
    print(f"\nNOT FOUND after {count} passwords")
    exit(1)
