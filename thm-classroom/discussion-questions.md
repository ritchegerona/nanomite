# Discussion Questions, Homework & Defense Cheat Sheet

## Wrap-up discussion questions (15 min)

### Conceptual
1. Why do attackers prefer stealing valid credentials over exploiting software bugs?
2. What makes a password "weak" in the context of offline cracking? (hint: entropy + hash speed)
3. Why is reusing the same password across sites dangerous even with strong passwords?
4. How does a hash protect a password at rest — and why doesn't it protect against cracking?
5. What is the difference between a phishing page stealing your password and malware reading it from memory?

### Technical
6. Name three places Windows stores (or leaks) credentials besides the SAM database.
7. Why is LSASS memory such a high-value target? What modern Windows feature protects it?
8. What does DPAPI do for browser-saved passwords, and what does an attacker need to unlock them?
9. In the lab, we found credentials in a PowerShell history file. What setting or policy reduces that risk?
10. What is Kerberoasting, and why does it not require administrator rights?

### Defensive thinking
11. Your company can only afford TWO of: password manager license, MFA rollout, SIEM monitoring. Which two and why?
12. A user insists on writing passwords in a notebook. Is that always wrong? When might it be acceptable?
13. How would you detect that someone is dumping LSASS on your network right now?
14. Why is "rotate passwords every 30 days" no longer recommended guidance (NIST SP 800-63B)?
15. What should a user do IMMEDIATELY if they suspect their password was stolen?

---

## Homework
Each student picks ONE defense control and prepares a 3-minute presentation:
- What it does (1 slide)
- Attack it prevents (1 slide)
- How to enable it (steps or screenshot)
- Limitations / bypasses to be aware of

Suggested controls: Windows Hello / MFA, password manager rollout, Windows LAPS,
Credential Guard, USB storage restriction, BitLocker, Sysmon + SIEM alerting,
browser password-breach alerts, phishing-resistant FIDO2 keys.

---

## Defense-controls cheat sheet (instructor reference)

| Attack (MITRE TA0006) | Primary Defense | Detection Signal |
|---|---|---|
| Clear-text creds in files | DLP policies, user training, forbid password storage in files | File audit on sensitive paths |
| Credentials in registry | App hardening, remove stored creds | `reg query` process audit (Sysmon EID 1) |
| SAM database extraction | Limit admin rights, LAPS, BitLocker + TPM | `vssadmin`/`reg save` command lines |
| LSASS memory dump | **Credential Guard**, LSASS PPL, EDR | Handle access to lsass.exe (Sysmon EID 10) |
| Kerberoasting | gMSA accounts, long random service passwords | Burst of TGS requests (EID 4769 w/ RC4) |
| DCSync | Restrict Replication rights, tiered admin | EID 4662 replication access by non-DC |
| NTDS.dit theft | Protect DCs, restrict logon rights, BitLocker | Volume-shadow/ntds file access |
| Browser password theft | OS auth to reveal, password manager, MFA | Odd extension installs, DPAPI access |
| Phishing credential capture | MFA / FIDO2, email filtering, user training | Impossible-travel logins, new-device alerts |
| Pass-the-Hash | LAPS, restrict local admin reuse | NTLM logons where interactive expected |

---

## Grading rubric (homework, 10 pts)
- Correct explanation of the control — 3
- Accurate attack mapping — 2
- Enablement steps verified — 2
- Limitations acknowledged — 2
- Clarity/presentation — 1
