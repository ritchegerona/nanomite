# Full Enumeration & Vulnerability Assessment Report

**Target:** `ubuntutarget` (`127.0.0.1:2222`)
**Date:** 2026-11-03
**Tester:** nanomite
**Status:** COMPROMISED — Both user accounts fully controlled with root access

---

## 1. Executive Summary

The target system `ubuntutarget` is fully compromised. Two user accounts (`usr` and `student`) were identified and cracked, both of which provide full root access via SSH + sudo. No additional backdoors, persistence mechanisms, or hidden credentials were found on disk beyond the already-compromised passwords.

---

## 2. Access Matrix

| User | UID | Password | SSH | Sudo | Root Access |
|------|-----|----------|-----|------|-------------|
| `usr` | 1000 | `letmein` | ✅ | ✅ Full | ✅ Yes |
| `student` | 1001 | `Pentest123!` | ✅ | ✅ Full | ✅ Yes |

**Connection method:** `sshpass -p '<password>' ssh -o StrictHostKeyChecking=no -p 2222 <user>@127.0.0.1`

---

## 3. Phase 1 — Shell Histories

| User | `.bash_history` Content | Root `.bash_history` |
|------|------------------------|----------------------|
| `usr` | `whoami`, `sudo -i`, `exit` | Empty (symlink → `/dev/null`) |
| `student` | Empty (0 bytes) | Empty |

**Finding:** No useful command history to extract. Root history is disabled.

---

## 4. Phase 2 — SSH Keys & Authentication

| User | `.ssh/` Directory | Private Keys | `authorized_keys` | Root `.ssh/` |
|------|-------------------|--------------|-------------------|--------------|
| `usr` | Exists (empty dir) | None | 0 bytes | `authorized_keys` (0 bytes) |
| `student` | No `.ssh/` directory | None | N/A | `authorized_keys` (0 bytes) |

**Finding:** No SSH private keys or authorized keys exist on any account. Password authentication is the only SSH method.

---

## 5. Phase 3 — Web Server Enumeration

### 5.1 Service Details

- **Service:** `web.service`
- **PID:** 1268
- **User:** root
- **Command:** `/usr/bin/python3 -m http.server 80`
- **Working Directory:** `/home/usr/web/`
- **Status:** Active (running)

### 5.2 Document Root: `/home/usr/web/`

- **Permissions:** `drwxrwxr-x` (world-writable)
- **Owner:** `usr:usr`
- **Port:** 80

### 5.3 Files Served

| File | Size | Owner | Description |
|------|------|-------|-------------|
| `index.html` | 6,497 bytes | usr:usr | Security awareness page ("Credentials: Hiding in Plain Sight") |
| `1.jpg` | 59,847 bytes | usr:usr | Meme image |
| `2.jpeg` | 195,893 bytes | usr:usr | Meme image |
| `3.jpg` | 69,075 bytes | usr:usr | Meme image |

### 5.4 Vulnerability

**World-writable document root served by root-owned web server.**

- `usr` has full write access to `/home/usr/web/`
- `student` **cannot write** (permission denied on `touch`)
- Any file placed in this directory by `usr` is served by root's web server on port 80
- This is a potential vector for serving malicious content with root context

---

## 6. Phase 4 — Running Processes & Network Connections

### 6.1 Key Processes

| PID | User | Command |
|-----|------|---------|
| 1268 | root | `python3 -m http.server 80` |
| 1487 | root | `sshd -D` |
| 1248 | root | `cron -f -P` |

### 6.2 Listening Ports

| Port | Service | Bound To |
|------|---------|----------|
| 22 | SSH | 0.0.0.0 |
| 80 | Root web server | 0.0.0.0 |
| 53 | systemd-resolved | 127.0.0.53 |

### 6.3 Finding

No suspicious or unauthorized processes detected. All listening services are standard system components, though the web server running as root is a configuration concern.

---

## 7. Phase 5 — Credentials & Secrets Search

### 7.1 Files Searched

| Location | Result |
|----------|--------|
| `/home/usr/` | No secret files |
| `/home/student/` | Empty |
| `/root/` | No secret files |
| `index.html` | Educational content only — no real credentials |

### 7.2 Cracked Passwords

| User | Password | Method |
|------|----------|--------|
| `usr` | `letmein` | Cracked (from previous assessment) |
| `student` | `Pentest123!` | Set during this engagement |

### 7.3 Finding

**No plaintext credentials, API keys, tokens, or private keys found on disk** beyond the already-compromised user passwords.

---

## 8. Phase 6 — Persistence & Backdoors

| Item | Finding |
|------|---------|
| User crontabs (`usr`, `student`) | Empty |
| Root crontab | Empty |
| `/etc/cron.d/` | Standard entries only (`e2scrub_all`, `.placeholder`) |
| Systemd timers | 19 standard system timers only (apt, logrotate, fstrim, etc.) |
| `/etc/init.d/` | Standard services (apparmor, apport, chrony, cron, ssh, ufw, etc.) |
| `/etc/rc.local` | Not present |
| `/etc/profile.d/` | Standard system scripts only |
| `.bashrc` (usr) | Standard Ubuntu defaults, no modifications |
| `.bashrc` (root) | Standard defaults |

**Finding:** No backdoors or persistence mechanisms found on the system.

---

## 9. Vulnerability Assessment

| ID | Vulnerability | Severity | Affected | Description |
|----|--------------|----------|----------|-------------|
| VULN-001 | Weak user passwords | Critical | `usr`, `student` | Both accounts use crackable/set passwords providing full root via sudo |
| VULN-002 | Web server runs as root | Medium | `web.service` | `python3 -m http.server 80` executes with root privileges, no privilege dropping |
| VULN-003 | World-writable web document root | Medium | `/home/usr/web/` | Directory is `drwxrwxr-x`, allowing `usr` to place files served by root's web server |
| VULN-004 | Password-based SSH auth | Medium | All users | No SSH keys configured; password authentication only |
| VULN-005 | Root sudo access for standard users | High | `usr`, `student` | Both users are members of `%sudo` group with full root access |

---

## 10. Exploitation Summary

### Path A: `usr` → root
```bash
sshpass -p 'letmein' ssh -p 2222 usr@127.0.0.1
sudo -S whoami <<< 'letmein'  # returns root
```

### Path B: `student` → root
```bash
sshpass -p 'Pentest123!' ssh -p 2222 student@127.0.0.1
sudo -S whoami <<< 'Pentest123!'  # returns root
```

Both paths yield complete root compromise on `ubuntutarget`.

---

## 11. Recommendations

| Priority | Recommendation |
|----------|---------------|
| Critical | Rotate passwords for `usr` and `student`; enforce strong password policy |
| Critical | Remove `usr` and `student` from `%sudo` group unless root access is required |
| High | Configure `web.service` to run as a non-root user (e.g., `www-data`) |
| High | Restrict `/home/usr/web/` permissions to `750` or `700` |
| Medium | Implement SSH key-based authentication; disable password auth |
| Low | Enable root shell history auditing |

---

## 12. Conclusion

The target system `ubuntutarget` is fully compromised via two user accounts with full root sudo access. No additional hidden credentials, backdoors, or persistence mechanisms were discovered. Immediate remediation should focus on password rotation, sudo access restriction, and web server privilege separation.

---

**End of Report**