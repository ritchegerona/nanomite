# TryHackMe Classroom Package — "How Credentials Get Stolen & How to Stop It"

Prepared for: educator-led session (90 minutes) + optional lab follow-up.

## Contents
| File | Purpose |
|------|---------|
| `lesson-plan.md` | Timed 90-minute session flow with room mappings |
| `slides.md` | Slide deck (paste into PowerPoint/Google Slides/reveal.js) |
| `discussion-questions.md` | Wrap-up discussion, homework, defense-controls cheat sheet |
| `educator-application.md` | How to get classroom seats + ready-to-send email template |

## Room curriculum (verified live)
### Module 1 — Foundations (all free)
- https://tryhackme.com/room/principlesofsecurity
- https://tryhackme.com/room/googledorking
- https://tryhackme.com/room/passwordcracking

### Module 2 — Credential harvesting (core demo)
- https://tryhackme.com/room/credharvesting — SAM, LSASS, Credential Manager, DCSync, LAPS (free room; prerequisites recommended: Active Directory Basics, Breaching AD)
- https://tryhackme.com/room/johntheripperbasics (free)

### Module 3 — Enterprise context
- https://tryhackme.com/room/winadbasics (free)
- https://tryhackme.com/room/attacktivedirectory (free)
- Module path: https://tryhackme.com/module/hacking-active-directory

## Key topics confirmed in the Credentials Harvesting room
- MITRE ATT&CK TA0006 (Credential Access) mapping
- Where credentials hide: clear-text files (PSReadLine history at
  `C:\Users\<u>\AppData\Roaming\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt`),
  registry (`reg query HKLM /f password /t REG_SZ /s`), database files, memory, password managers, AD
- LSASS memory dumping, SAM extraction, Windows Credential Manager
- Domain Controller: NTDS.dit, SYSVOL/GPP encryption keys, DCSync
- LAPS enumeration; AD attacks leading to credential exposure
- Credential forms: passwords, NTLM hashes, TGT/TGS tickets, private keys

## Suggested next steps
1. Send the educator email template (see `educator-application.md`) — free 14-day Education trial
2. Run Password Cracking + Credentials Harvesting yourself first to verify timings
3. Print/assign `discussion-questions.md` for the wrap-up
4. Do NOT share walkthrough links (several exist on GitHub/YouTube and spoil the rooms)
