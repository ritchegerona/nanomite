import msoffcrypto
import io

filepath = "/Users/ritchegerona/FILES/MSR FILES/SAFEWAY/2026/MEDICAL STAFFING (MSR)_1-9-2026.xlsx"
output = "/Users/ritchegerona/nanomite/MEDICAL_STAFFING_MSR_1-9-2026_decrypted.xlsx"

with open(filepath, "rb") as f:
    office_file = msoffcrypto.OfficeFile(io.BytesIO(f.read()))

office_file.load_key(password="123456")

decrypted = io.BytesIO()
office_file.decrypt(decrypted)

with open(output, "wb") as f:
    f.write(decrypted.getvalue())

print(f"Decrypted file written to: {output}")
print(f"Size: {len(decrypted.getvalue())} bytes")
