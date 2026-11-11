# Slides — "How Credentials Get Stolen & How to Stop It"

Paste into PowerPoint / Google Slides / Marp / reveal.js.
One block = one slide.

---

## 1. Title
**How Credentials Get Stolen — and How to Stop It**
A hands-on introduction to credential attacks and defenses
*Instructor name · Course · Date*

---

## 2. Why attackers want YOUR password
- Credentials beat exploits: no vulnerability needed
- One password → email → bank → whole company
- Stolen credentials are used in the majority of breaches
- MITRE ATT&CK tactic **TA0006: Credential Access**

---

## 3. What counts as a "credential"
- Username + password
- NTLM hashes (Windows)
- Kerberos tickets (TGT / TGS)
- Private keys, API tokens, session cookies

---

## 4. Where credentials hide on a system
- Clear-text files: command history (`ConsoleHost_history.txt`), configs, backups
- Registry: `reg query HKLM /f password /t REG_SZ /s`
- Database files (apps store settings + creds)
- Memory (LSASS): running passwords & tickets
- Password managers & browser stores
- Active Directory: user descriptions, SYSVOL, NTDS.dit

---

## 5. Demo: hashing vs. cracking
- Hashing is one-way: password → hash (cannot reverse directly)
- Cracking = guessing candidates and comparing hashes
- Tools: John the Ripper, Hashcat
- Weakness exploited: weak/short/reused passwords
- Live demo: crack an MD5 hash in < 1 minute

---

## 6. Demo: Windows credential storage
- SAM database → local account hashes
- LSASS process memory → clear-text creds, tickets
- DPAPI protects browser/credential-manager secrets (tied to Windows user)
- Demo: navigate the lab VM, find the "password" key in registry

---

## 7. When one password compromises a company
- Active Directory: single domain credential → lateral movement
- Kerberoasting: request service tickets → crack offline
- DCSync: ask the DC to replicate all hashes
- Pass-the-Hash: use the hash without ever knowing the password

---

## 8. Defenses that actually work
- **MFA everywhere** — stops most credential abuse outright
- **Password managers** — unique, long passwords per site
- **LAPS / Windows LAPS** — unique local admin password per machine
- **Credential Guard** — protects LSASS memory
- **Disable WDigest/clear-text** — no plaintext in memory
- **Monitoring** — alert on LSASS access, Kerberoasting, DCSync
- **Least privilege** — limit who can dump memory at all

---

## 9. Detection ideas (blue-team corner)
- Suspicious access to `lsass.exe` → Event ID 4656/4663
- Many TGS requests in short time → Kerberoasting
- DCSync pattern → Directory Replication rights misuse (4662)
- PowerShell history file access from unusual processes

---

## 10. Key takeaways
1. Attackers prefer passwords over exploits
2. Credentials hide in more places than you think
3. Weak passwords die in seconds to offline cracking
4. MFA + unique passwords + monitoring = strong baseline
5. Defense is a process, not a product

---

## 11. Homework
Document ONE defense control: what it does, how to enable it, what attack it stops.
(presentations next session)
