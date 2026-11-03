# Enumeration Report Capability

This document describes the full system enumeration and vulnerability assessment report capability added to the nanomite repository.

## Overview

The report capability provides a complete security assessment of compromised target systems, including shell history extraction, SSH key enumeration, web server analysis, process and network inspection, credential harvesting, persistence detection, vulnerability assessment, and exploitation path documentation.

## Report Files

All report artifacts are located in `report/`:

| File | Format | Description |
|------|--------|-------------|
| `CTIA_FULL_ENUMERATION_REPORT.md` | Markdown | Source report in CTIA (Cyber Threat Intelligence Analysis) format |
| `CTIA_FULL_ENUMERATION_REPORT.pdf` | PDF | Rendered PDF version of the report |
| `CTIA_FULL_ENUMERATION_REPORT.docx` | Microsoft Word | Professional Word document with styled tables and formatting |
| `generate_report.py` | Python | Report generator script for regenerating artifacts |
| `README.md` | Markdown | Quick-start guide for the report capability |

## Target System Details

| Field | Value |
|-------|-------|
| Target | `ubuntutarget` (`127.0.0.1:2222`) |
| OS | Ubuntu Server |
| Compromised Accounts | `usr` (`letmein`), `student` (`Pentest123!`) |
| Access Level | Full root via SSH + sudo |
| Web Service | Root-owned Python HTTP server on port 80 |
| Document Root | `/home/usr/web/` (world-writable) |

## Report Contents

The report includes 17 sections:

1. Executive Summary
2. Classification & Distribution
3. Target Information
4. Access Matrix
5. Shell History Enumeration
6. SSH Key Enumeration
7. Web Server Enumeration
8. Process & Network Enumeration
9. Credential & Secret Harvesting
10. Persistence & Backdoor Analysis
11. Vulnerability Assessment (VULN-001 through VULN-006)
12. MITRE ATT&CK Mapping
13. Exploitation Paths
14. Risk Analysis
15. Remediation Recommendations
16. Conclusion
17. Appendices

## Vulnerabilities Identified

| ID | Severity | Description |
|----|----------|-------------|
| VULN-001 | Critical | Weak password: `usr` / `letmein` |
| VULN-002 | Critical | Weak password: `student` / `Pentest123!` |
| VULN-003 | High | Unrestricted sudo access for both users |
| VULN-004 | Medium | Web service running as root |
| VULN-005 | Medium | World-writable web document root |
| VULN-006 | Low | Password-only SSH authentication (no MFA) |

## Regenerating Reports

```bash
cd report/
python3 generate_report.py
```

## GitHub Pages

Reports are committed to the `master` branch of `ritchegerona/nanomite`. GitHub Pages can be enabled in repository settings to serve the markdown report directly.
