[Your Name / Organization]  
[Your Address]  
[Email] | [Phone]  
[Date]

To:  
**Information Security / CISO Office**  
Department of Health (DOH)  
San Lazaro Compound, Tayuman, Sta. Cruz, Manila  
cc: DICT – CERT-PH (if applicable)

Subject: **Request for Authorization – Web Application Security Assessment (doh.gov.ph and Regional Subdomains)**

Dear Sir/Madam,

I am writing to request formal written authorization to conduct a limited, non-destructive web application security assessment of select Department of Health digital assets, specifically including `ro7.doh.gov.ph` (DOH Central Visayas) and related `doh.gov.ph` subdomains.

### 1. Purpose
Recent passive reconnaissance identified that `ro7.doh.gov.ph` is running a Joomla content management system exposed behind an Azure WAF. The purpose of this assessment is to help DOH identify and remediate potential authentication and configuration weaknesses before they can be exploited by malicious actors.

### 2. Proposed Scope
**In scope:**
- `https://ro7.doh.gov.ph/administrator/` (Joomla admin login panel)
- Related Joomla administrative endpoints on the same host

**Out of scope:**
- Denial-of-service testing
- Brute-force or password-spraying against user accounts
- Database injection or data exfiltration
- Testing of production user data or patient records
- Any system outside the `doh.gov.ph` domain

### 3. Proposed Methodology
With authorization, I intend to:
- Verify whether default or weak Joomla administrator credentials are in use (`admin` / common defaults) using a **single, low-frequency login attempt** only.
- Confirm Joomla version and check for known, publicly documented vulnerabilities.
- Validate HTTP security headers (e.g., `Secure`, `HttpOnly` flags on session cookies).
- All testing will be performed from a single, static IP address provided upon approval.

### 4. Safety Measures
- Testing will be limited to business hours Philippines Standard Time (PHT).
- No automated scanners, brute-force tools, or credential stuffing will be used without prior written approval.
- No data will be modified, deleted, or exfiltrated.
- If valid credentials are discovered, testing will stop immediately and findings will be reported.

### 5. Point of Contact
[Your Name]  
[Organization / Independent Researcher]  
[Phone]  
[Email]  
Scanner source IP: [Your IP]

### 6. Reporting & Disclosure
All findings will be shared privately with DOH first, with a recommended 90-day remediation window before any public disclosure, consistent with responsible disclosure practices.

I am happy to provide a signed NDA, proof of identity, or a more detailed rules-of-engagement document if required.

Please confirm authorization by replying to this email or contacting me at the details above.

Respectfully,

[Your Signature]  
[Your Printed Name]  
[Title / Affiliation]