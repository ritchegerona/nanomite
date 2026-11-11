# Lesson Plan — 90-minute session
## "How Credentials Get Stolen & How to Stop It"

**Audience:** students new to offensive security concepts
**Format:** instructor-led demo + hands-on student labs
**Platform:** TryHackMe (free rooms unless noted)

---

## Objectives
By the end of the session, students can:
1. Explain why credentials are the primary attack target (vs. exploits)
2. Identify where credentials are stored on Windows systems (files, registry, memory, AD)
3. Perform basic hash cracking with John the Ripper / Hashcat
4. Map attacks to MITRE ATT&CK TA0006 (Credential Access)
5. Name at least 5 defensive controls that stop credential theft

---

## Session flow (90 min)

| # | Time | Segment | Activity | Room/Resource |
|---|------|---------|----------|---------------|
| 1 | 0:00–0:10 | Why credentials are the crown jewels | Slide talk + Q&A | slides.md §1–2 |
| 2 | 0:10–0:25 | Live demo: hash cracking | Project Password Cracking room; crack a hash in real time | tryhackme.com/room/passwordcracking |
| 3 | 0:25–0:50 | Hands-on: credential harvesting | Students in pairs deploy room VM and work Tasks 1–4 | tryhackme.com/room/credharvesting |
| 4 | 0:50–1:10 | Where credentials hide on Windows | Slide talk: SAM, LSASS, DPAPI, Credential Manager, PSReadLine history | slides.md §4–5 |
| 5 | 1:10–1:25 | Wrap-up + defense discussion | discussion-questions.md | — |
| 6 | 1:25–1:30 | Homework assignment | Each student documents one defense control | discussion-questions.md §Homework |

---

## Instructor prep checklist
- [ ] TryHackMe account created; rooms completed once for timing
- [ ] Educator/trial seats arranged (see educator-application.md) — optional but recommended for AD rooms
- [ ] Projector + Zoom/share link tested
- [ ] Student pairs pre-assigned
- [ ] Walkthrough links NOT shared (GitHub/YouTube solutions exist — they spoil the labs)
- [ ] Backup plan: if room VM is slow, switch to instructor-demo-only mode

## Optional extended path (multi-week)
- Week 1: Principles of Security + Google Dorking
- Week 2: Password Cracking + John The Ripper Basics
- Week 3: Active Directory Basics (winadbasics)
- Week 4: Credentials Harvesting (credharvesting)
- Week 5: Attacktive Directory (Kerberoasting, AS-REP)
- Week 6: Wrap-up + student presentations of defense controls
