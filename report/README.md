# Report Generation Capability

This directory contains the full enumeration and vulnerability assessment reports for the `ubuntutarget` (`127.0.0.1:2222`) security engagement, plus the generator script used to produce them.

## Files

| File | Description |
|------|-------------|
| `FULL_ENUMERATION_REPORT.md` | Source markdown report (all 12 enumeration phases, vulnerability assessment, exploitation paths, recommendations) |
| `FULL_ENUMERATION_REPORT.pdf` | Rendered PDF version of the report |
| `FULL_ENUMERATION_REPORT.docx` | Professional Word document version with styled tables, severity color-coding, headers/footers |
| `generate_report.py` | Python script that converts the markdown source into PDF and DOCX formats |
| `README.md` | This documentation file |

## Report Contents

The report covers:

1. **Executive Summary** — Overall findings and risk posture
2. **Access Matrix** — Credentials and access levels for `usr` and `student`
3. **Phase 1: Shell History** — Command history for both accounts
4. **Phase 2: SSH Keys** — SSH key and authorized_keys inventory
5. **Phase 3: Web Server Enumeration** — `web.service` (root, port 80), document root, served files
6. **Phase 4: Processes & Network** — Running processes, listening ports (22, 80, 53)
7. **Phase 5: Credentials & Secrets** — Credential harvesting results
8. **Phase 6: Persistence & Backdoors** — Cron, systemd timers, init scripts, rc.local
9. **Vulnerability Assessment** — VULN-001 through VULN-005 with CVSS scores
10. **Exploitation Paths** — Step-by-step attack chains
11. **Recommendations** — Prioritized remediation
12. **Conclusion** — Final security posture summary

## Regenerating Reports

```bash
cd /home/ritche/mailforen/report
python3 generate_report.py
```

Dependencies: `weasyprint` (PDF), `python-docx` (DOCX).

## Target Summary

| Asset | Value |
|-------|-------|
| Target | `ubuntutarget` (`127.0.0.1:2222`) |
| Accounts | `usr` (`letmein`), `student` (`Pentest123!`) |
| Access | Full root via SSH + sudo on both accounts |
| Critical Issues | Weak passwords, root-run web server, world-writable document root, password-only SSH |