//! Advanced Evasion: WAF bypass catalog, EDR telemetry analysis, living-off-the-land techniques.
//!
//! All content is DEFENSIVE/DETECTION-FOCUSED - catalogs techniques for building
//! detection rules and validating defensive coverage. No weaponized payloads.

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// WAF bypass technique entry (detection-focused).
#[derive(Debug, Clone)]
pub struct WafBypassTechnique {
    pub name: String,
    pub category: String, // SQLi, XSS, RCE, Path Traversal, etc.
    pub description: String,
    pub example_payload: String, // Sanitized/placeholder only
    pub detection: String,
    pub mitigation: String,
}

/// Returns a catalog of WAF bypass techniques for detection engineering.
pub fn waf_bypass_catalog() -> ModuleReport {
    let techniques = vec![
        WafBypassTechnique {
            name: "SQLi: Comment Termination".to_string(),
            category: "SQL Injection".to_string(),
            description: "Terminate query with comment to bypass authentication/logic".to_string(),
            example_payload: "' -- / # / ;--".to_string(),
            detection: "SQL meta-characters (' -- ; #) in input, unusual query patterns in WAF logs".to_string(),
            mitigation: "Parameterized queries, WAF rule for comment sequences, input validation".to_string(),
        },
        WafBypassTechnique {
            name: "SQLi: Union-Based Extraction".to_string(),
            category: "SQL Injection".to_string(),
            description: "UNION SELECT to extract data from other tables".to_string(),
            example_payload: "' UNION SELECT null,version(),null --".to_string(),
            detection: "UNION keyword in input, mismatched column counts in errors, version() calls".to_string(),
            mitigation: "Parameterized queries, column count validation, error suppression".to_string(),
        },
        WafBypassTechnique {
            name: "SQLi: Blind/Time-Based".to_string(),
            category: "SQL Injection".to_string(),
            description: "Boolean or time-based inference when no direct output".to_string(),
            example_payload: "' AND (SELECT CASE WHEN (1=1) THEN pg_sleep(5) ELSE pg_sleep(0) END) --".to_string(),
            detection: "pg_sleep/waitfor/delay functions, response time anomalies, boolean logic in input".to_string(),
            mitigation: "Parameterized queries, response time monitoring, WAF time-based rules".to_string(),
        },
        WafBypassTechnique {
            name: "SQLi: Encoding/Obfuscation".to_string(),
            category: "SQL Injection".to_string(),
            description: "URL encoding, double encoding, Unicode, hex, char() to evade signatures".to_string(),
            example_payload: "%27%20%55%4e%49%4f%4e / char(85)+char(78)+...".to_string(),
            detection: "High entropy input, mixed encoding layers, decode-then-inspect pipeline".to_string(),
            mitigation: "Normalize/decoded input before WAF inspection, charset allow-lists".to_string(),
        },
        WafBypassTechnique {
            name: "SQLi: HTTP Parameter Pollution".to_string(),
            category: "SQL Injection".to_string(),
            description: "Duplicate parameters to confuse WAF parsing vs backend parsing".to_string(),
            example_payload: "?id=1&id=2 UNION SELECT...".to_string(),
            detection: "Duplicate parameter names, different WAF vs backend parameter handling".to_string(),
            mitigation: "Reject duplicate parameters, consistent parameter parsing".to_string(),
        },
        WafBypassTechnique {
            name: "XSS: Event Handlers".to_string(),
            category: "Cross-Site Scripting".to_string(),
            description: "HTML event attributes to execute JS without <script>".to_string(),
            example_payload: "<img src=x onerror=alert(1)> / <svg onload=...>".to_string(),
            detection: "onerror/onload/onmouseover/etc in input, HTML tags in non-HTML contexts".to_string(),
            mitigation: "CSP with script-src 'self', input sanitization, output encoding".to_string(),
        },
        WafBypassTechnique {
            name: "XSS: JavaScript Protocol".to_string(),
            category: "Cross-Site Scripting".to_string(),
            description: "javascript: URLs in href/src attributes".to_string(),
            example_payload: "<a href=javascript:alert(1)>click</a>".to_string(),
            detection: "javascript: protocol in attributes, CSP violation reports".to_string(),
            mitigation: "CSP, sanitize URLs, allow-list protocols (https: only)".to_string(),
        },
        WafBypassTechnique {
            name: "XSS: Template Injection (SSTI)".to_string(),
            category: "Server-Side Template Injection".to_string(),
            description: "Template syntax in user input rendered server-side (Jinja2, Twig, Freemarker)".to_string(),
            example_payload: "{{7*7}} / ${7*7} / #{7*7} / <%= 7*7 %>".to_string(),
            detection: "Template syntax in input, unexpected math/string output in response".to_string(),
            mitigation: "Sandbox template engine, disable unsafe functions, input validation".to_string(),
        },
        WafBypassTechnique {
            name: "XSS: Mutation XSS (mXSS)".to_string(),
            category: "Cross-Site Scripting".to_string(),
            description: "Browser parser mutation turns safe markup into executable".to_string(),
            example_payload: "<svg><p><style><img src=xxx:onerror=alert(1)>".to_string(),
            detection: "Complex nested markup, parser differential analysis".to_string(),
            mitigation: "DOMPurify with SAFE_FOR_TEMPLATES, CSP, avoid innerHTML with user input".to_string(),
        },
        WafBypassTechnique {
            name: "RCE: Command Injection".to_string(),
            category: "Remote Code Execution".to_string(),
            description: "Shell metacharacters in command arguments (; & | ` $() )".to_string(),
            example_payload: "; id / && cat /etc/passwd / | nslookup attacker.com".to_string(),
            detection: "Shell metacharacters in input, process execution from web server".to_string(),
            mitigation: "Avoid shell commands, use execve with args array, allow-list commands".to_string(),
        },
        WafBypassTechnique {
            name: "RCE: Deserialization".to_string(),
            category: "Remote Code Execution".to_string(),
            description: "Unsafe deserialization of user-controlled data (Java, .NET, PHP, Python)".to_string(),
            example_payload: "serialized gadget chain (ysoserial, GadgetProbe)".to_string(),
            detection: "Known magic bytes (AC ED 00 05, etc), unexpected object types, class loading anomalies".to_string(),
            mitigation: "Avoid deserialization of untrusted data, use safe formats (JSON), integrity checks".to_string(),
        },
        WafBypassTechnique {
            name: "RCE: Path Traversal".to_string(),
            category: "Path Traversal".to_string(),
            description: "Directory traversal to read/write arbitrary files (../..)".to_string(),
            example_payload: "../../etc/passwd / ..%2f..%2fetc%2fpasswd / %2e%2e%2f".to_string(),
            detection: "Path traversal sequences, file access outside web root, canonicalization bypass".to_string(),
            mitigation: "Path canonicalization, chroot/jail, allow-list file access, WAF traversal rules".to_string(),
        },
        WafBypassTechnique {
            name: "RCE: XXE (XML External Entity)".to_string(),
            category: "XXE".to_string(),
            description: "External entity references in XML to read files/SSRF".to_string(),
            example_payload: "<!DOCTYPE foo [<!ENTITY xxe SYSTEM 'file:///etc/passwd'>]><foo>&xxe;</foo>".to_string(),
            detection: "DOCTYPE declarations, ENTITY references, SYSTEM/PUBLIC identifiers in XML".to_string(),
            mitigation: "Disable external entities, use safe XML parsers, WAF XXE rules".to_string(),
        },
        WafBypassTechnique {
            name: "Protocol Smuggling (HTTP/2, HTTP/1.1)".to_string(),
            category: "Protocol".to_string(),
            description: "Request smuggling via CL.TE, TE.CL, TE.TE desync".to_string(),
            example_payload: "Transfer-Encoding: chunked + Content-Length mismatch".to_string(),
            detection: "Conflicting CL/TE headers, response splitting, cache poisoning indicators".to_string(),
            mitigation: "Normalize headers, reject ambiguous requests, use HTTP/2 end-to-end".to_string(),
        },
        WafBypassTechnique {
            name: "Header Injection / Response Splitting".to_string(),
            category: "Header".to_string(),
            description: "CRLF in headers to inject response headers/body".to_string(),
            example_payload: "Header: value\r\nInjected-Header: evil".to_string(),
            detection: "CRLF in header values, multiple Set-Cookie, unexpected response structure".to_string(),
            mitigation: "Header validation, strip CRLF, use header APIs not string concat".to_string(),
        },
        WafBypassTechnique {
            name: "Race Condition / TOCTOU".to_string(),
            category: "Logic".to_string(),
            description: "Time-of-check-time-of-use in file/state operations".to_string(),
            example_payload: "Parallel requests to bypass checks".to_string(),
            detection: "High concurrent requests to same endpoint, state inconsistency logs".to_string(),
            mitigation: "Atomic operations, file locking, idempotency keys, serializable transactions".to_string(),
        },
        WafBypassTechnique {
            name: "WAF Evasion: Fragmentation".to_string(),
            category: "Evasion".to_string(),
            description: "Split payload across multiple packets/requests to evade inspection".to_string(),
            example_payload: "POST body split, HTTP/2 frames, chunked encoding abuse".to_string(),
            detection: "Reassembly at application layer, stream inspection, anomaly scoring".to_string(),
            mitigation: "Full request reassembly before inspection, stream-based WAF".to_string(),
        },
        WafBypassTechnique {
            name: "WAF Evasion: Case/Whitespace Variation".to_string(),
            category: "Evasion".to_string(),
            description: "Case variation, alternative whitespace, null bytes to bypass regex".to_string(),
            example_payload: "SeLeCt / Se\tlect / Sel%00ect".to_string(),
            detection: "Normalize case/whitespace before pattern matching, null byte stripping".to_string(),
            mitigation: "Normalize input before WAF rules, allow-list character sets".to_string(),
        },
    ];

    let mut report = ModuleReport::new(
        "advanced_evasion",
        "waf-bypass",
        "catalog",
        format!(
            "{} WAF bypass techniques for detection engineering",
            techniques.len()
        ),
    );

    for t in &techniques {
        report = report.line(format!("[{}] {}", t.category, t.name));
        report = report.line(format!("  Description: {}", t.description));
        report = report.line(format!("  Example (sanitized): {}", t.example_payload));
        report = report.line(format!("  Detection: {}", t.detection));
        report = report.line(format!("  Mitigation: {}", t.mitigation));
        report = report.line("");
    }

    report = report.finding(
        Finding::new(
            "advanced_evasion",
            "waf-bypass",
            "catalog",
            "WAF bypass technique catalog for rule development",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail("Use to test WAF rules, build detection signatures, and validate coverage"),
    );

    report
}

/// EDR/AV telemetry and detection evasion techniques (detection-focused).
#[derive(Debug, Clone)]
pub struct EdrEvasionTechnique {
    pub name: String,
    pub category: String,
    pub description: String,
    pub telemetry_sources: Vec<String>,
    pub detection: String,
    pub mitigation: String,
}

/// Returns EDR evasion techniques with telemetry sources for detection engineering.
pub fn edr_evasion_catalog() -> ModuleReport {
    let techniques = vec![
        EdrEvasionTechnique {
            name: "Direct Syscalls / SysWhispers".to_string(),
            category: "Execution".to_string(),
            description: "Bypass userland hooks by issuing syscalls directly from assembly".to_string(),
            telemetry_sources: vec!["Kernel callback (PsSetCreateProcessNotifyRoutine)", "ETW (Microsoft-Windows-Kernel-Process)", "Sysmon Event 1/8/10"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Process creation without corresponding userland API calls, unusual call stacks".to_string(),
            mitigation: "Kernel-mode monitoring, syscall anomaly detection, enable Sysmon".to_string(),
        },
        EdrEvasionTechnique {
            name: "DLL Unhooking / Module Stomping".to_string(),
            category: "Defense Evasion".to_string(),
            description: "Restore original ntdll.dll from disk to remove EDR hooks".to_string(),
            telemetry_sources: vec!["File read of ntdll.dll (Sysmon 8)", "Memory protection changes (VirtualProtect)", "Module load events"].into_iter().map(|s| s.to_string()).collect(),
            detection: "ntdll.dll read from disk by non-system process, memory permission changes on ntdll".to_string(),
            mitigation: "Monitor ntdll.dll access, kernel callbacks on memory protection changes".to_string(),
        },
        EdrEvasionTechnique {
            name: "ETW Tampering / Patchless AMSI Bypass".to_string(),
            category: "Defense Evasion".to_string(),
            description: "Disable ETW providers or patch AMSI in-memory (AmsiScanBuffer)".to_string(),
            telemetry_sources: vec!["ETW provider state changes", "AMSI DLL load (amsi.dll)", "PowerShell script block logging (4104)"].into_iter().map(|s| s.to_string()).collect(),
            detection: "ETW provider disable, AMSI initialization failure, modified AmsiScanBuffer".to_string(),
            mitigation: "Kernel ETW monitoring, AMSI health checks, script block logging".to_string(),
        },
        EdrEvasionTechnique {
            name: "PPL (Protected Process Light) Bypass".to_string(),
            category: "Privilege Escalation".to_string(),
            description: "Abuse PPL hierarchy to access LSASS (e.g., PPL dumper, custom driver)".to_string(),
            telemetry_sources: vec!["Driver load (Sysmon 6)", "Process open with PROCESS_VM_READ on LSASS", "SeDebugPrivilege use"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Non-Microsoft signed driver, LSASS access from unusual process, PPL level mismatch".to_string(),
            mitigation: "Driver signature enforcement, Credential Guard, LSASS PPL (RunAsPPL)".to_string(),
        },
        EdrEvasionTechnique {
            name: "Living Off The Land Binaries (LOLBins)".to_string(),
            category: "Execution".to_string(),
            description: "Use signed Microsoft binaries for execution (msbuild, csc, regsvr32, rundll32, etc.)".to_string(),
            telemetry_sources: vec!["Process creation (Sysmon 1)", "Command line logging (4688)", "DLL load (Sysmon 7)", "Network connections (Sysmon 3)"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Unusual parent-child (word.exe -> cmd.exe), network from LOLBin, args with URLs/scripts".to_string(),
            mitigation: "AppLocker/WDAC, command line logging, parent-child anomaly detection".to_string(),
        },
        EdrEvasionTechnique {
            name: "Living Off The Land Scripts (LOLScripts)".to_string(),
            category: "Execution".to_string(),
            description: "Use built-in scripting (PowerShell, WScript, CScript, MSHTA, JScript/VBScript)".to_string(),
            telemetry_sources: vec!["Script block logging (4104)", "Module logging (4103)", "Process creation", "AMSI scan results"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Obfuscated scripts, download cradles (IEX/Invoke-Expression), encoded commands".to_string(),
            mitigation: "Constrained Language Mode, AMSI, script block logging, disable WSH if unused".to_string(),
        },
        EdrEvasionTechnique {
            name: "Process Injection (Classic)".to_string(),
            category: "Privilege Escalation / Defense Evasion".to_string(),
            description: "OpenProcess -> VirtualAllocEx -> WriteProcessMemory -> CreateRemoteThread".to_string(),
            telemetry_sources: vec!["Process open (VM_WRITE/VM_OPERATION)", "Remote thread creation (Sysmon 8)", "Memory allocation in remote process"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Cross-process memory ops, remote thread in critical process (lsass, svchost)".to_string(),
            mitigation: "Block remote thread creation, monitor OpenProcess with suspicious access".to_string(),
        },
        EdrEvasionTechnique {
            name: "Process Injection (Advanced: Thread Hijacking, APC, Proc Thread Attribute)".to_string(),
            category: "Privilege Escalation / Defense Evasion".to_string(),
            description: "QueueUserAPC, SetThreadContext, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, Early Bird".to_string(),
            telemetry_sources: vec!["Thread context modification", "APC queueing", "Process creation with spoofed parent"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Thread context changes in remote process, APC to unusual threads, PPID spoofing".to_string(),
            mitigation: "Kernel callbacks on thread ops, PPID validation, ETW thread events".to_string(),
        },
        EdrEvasionTechnique {
            name: "Memory Module / Reflective DLL Injection".to_string(),
            category: "Defense Evasion".to_string(),
            description: "Load DLL from memory without disk artifact (reflective loader, Donut)".to_string(),
            telemetry_sources: vec!["Memory allocation (RWX)", "Module load without disk file (Sysmon 7)", "VAD anomalies"].into_iter().map(|s| s.to_string()).collect(),
            detection: "RWX memory regions, modules not on disk, memory scanner (Moneta, HollowsHunter)".to_string(),
            mitigation: "Memory scanning, block RWX, monitor VirtualAllocEx with PAGE_EXECUTE_READWRITE".to_string(),
        },
        EdrEvasionTechnique {
            name: "Credential Theft: LSASS Dump Variants".to_string(),
            category: "Credential Access".to_string(),
            description: "comsvcs.dll MiniDump, werfault.exe, procdump, sqldumper, custom minidump".to_string(),
            telemetry_sources: vec!["LSASS handle with VM_READ", "Minidump file creation", "werfault/comsvcs/procdump execution"].into_iter().map(|s| s.to_string()).collect(),
            detection: "LSASS read access, dump file written, known dumper binaries".to_string(),
            mitigation: "Credential Guard, RunAsPPL, monitor LSASS handles, block known dumpers".to_string(),
        },
        EdrEvasionTechnique {
            name: "WMI / COM / DCOM Lateral Movement".to_string(),
            category: "Lateral Movement".to_string(),
            description: "WMI (Win32_Process.Create), DCOM (ShellWindows, MMC20), COM hijacking".to_string(),
            telemetry_sources: vec!["WmiPrvSE process creation", "DCOM activation (Event 10016)", "Remote RPC calls"].into_iter().map(|s| s.to_string()).collect(),
            detection: "WmiPrvSE spawning unusual children, DCOM from non-admin workstation".to_string(),
            mitigation: "Restrict WMI/DCOM, monitor WmiPrvSE children, network segmentation".to_string(),
        },
        EdrEvasionTechnique {
            name: "Kernel Exploits / Driver Load".to_string(),
            category: "Privilege Escalation".to_string(),
            description: "Vulnerable driver (BYOVD), CVE exploits, signed driver abuse".to_string(),
            telemetry_sources: vec!["Driver load (Sysmon 6)", "Kernel exploit artifacts", "BSOD/crash dumps"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Unsigned/expired driver, known vulnerable driver hashes, kernel pool corruption".to_string(),
            mitigation: "Driver signature enforcement, HVCI/VBS, block known vulnerable drivers (Microsoft list)".to_string(),
        },
        EdrEvasionTechnique {
            name: "Timestomping / File Manipulation".to_string(),
            category: "Defense Evasion".to_string(),
            description: "Modify file timestamps (Create/Modify/Access) to blend in".to_string(),
            telemetry_sources: vec!["File time changes (Sysmon 2/26)", "USN journal", "File hash vs timestamp mismatch"].into_iter().map(|s| s.to_string()).collect(),
            detection: "Timestamp anomalies (future dates, identical times), USN journal mismatch".to_string(),
            mitigation: "File integrity monitoring (FIM), USN journal collection, hash-based allow-lists".to_string(),
        },
        EdrEvasionTechnique {
            name: "Network Evasion: Domain Fronting / CDN".to_string(),
            category: "Command & Control".to_string(),
            description: "Route C2 through high-reputation CDN (Cloudflare, CloudFront, Azure Front Door)".to_string(),
            telemetry_sources: vec!["DNS resolution to CDN", "TLS SNI vs Host header mismatch", "JA3 fingerprint"].into_iter().map(|s| s.to_string()).collect(),
            detection: "SNI/Host mismatch, known C2 JA3, traffic to CDN with unusual patterns".to_string(),
            mitigation: "TLS inspection, JA3 allow-lists, DNS sinkholing, CDN access logging".to_string(),
        },
    ];

    let mut report = ModuleReport::new(
        "advanced_evasion",
        "edr-evasion",
        "catalog",
        format!(
            "{} EDR evasion techniques with telemetry sources",
            techniques.len()
        ),
    );

    for t in &techniques {
        report = report.line(format!("[{}] {}", t.category, t.name));
        report = report.line(format!("  Description: {}", t.description));
        report = report.line("  Telemetry Sources:".to_string());
        for src in &t.telemetry_sources {
            report = report.line(format!("    - {}", src));
        }
        report = report.line(format!("  Detection: {}", t.detection));
        report = report.line(format!("  Mitigation: {}", t.mitigation));
        report = report.line("");
    }

    report = report.finding(
        Finding::new("advanced_evasion", "edr-evasion", "catalog",
            "EDR evasion technique catalog with telemetry mapping",
            RiskLevel::Informational)
        .confidence(Confidence::Confirmed)
        .detail("Map telemetry sources to detection rules; prioritize kernel/ETW sources that can't be userland-tampered"),
    );

    report
}

/// LOLBin/LOLScript catalog with MITRE mapping.
#[derive(Debug, Clone)]
pub struct LolBin {
    pub binary: String,
    pub mitre_ids: Vec<String>,
    pub description: String,
    pub typical_abuse: String,
    pub detection: String,
}

/// Returns a catalog of Living Off The Land binaries and scripts.
pub fn lolbin_catalog() -> ModuleReport {
    let bins = vec![
        LolBin {
            binary: "msbuild.exe".to_string(),
            mitre_ids: vec!["T1127.001".into()],
            description: "Build engine, executes inline tasks/C#".to_string(),
            typical_abuse: "C# code execution via XML project".to_string(),
            detection: "msbuild from non-devenv parent, network connections, unusual .xml/.csproj"
                .to_string(),
        },
        LolBin {
            binary: "csc.exe".to_string(),
            mitre_ids: vec!["T1127.001".into()],
            description: "C# compiler".to_string(),
            typical_abuse: "Compile and execute C# on the fly".to_string(),
            detection: "csc.exe from temp, network, non-developer context".to_string(),
        },
        LolBin {
            binary: "regsvr32.exe".to_string(),
            mitre_ids: vec!["T1218.010".into()],
            description: "Register COM DLLs".to_string(),
            typical_abuse: "Execute code via DllRegisterServer (sct, dll)".to_string(),
            detection: "regsvr32 with /s /n /u /i, network, non-system32".to_string(),
        },
        LolBin {
            binary: "rundll32.exe".to_string(),
            mitre_ids: vec!["T1218.011".into()],
            description: "Run DLL exports".to_string(),
            typical_abuse: "Execute exported function from DLL".to_string(),
            detection: "rundll32 with non-CPL entry, network, unusual parent".to_string(),
        },
        LolBin {
            binary: "mshta.exe".to_string(),
            mitre_ids: vec!["T1218.005".into()],
            description: "HTA execution".to_string(),
            typical_abuse: "Run HTML Application with VBScript/JS".to_string(),
            detection: "mshta from temp/Downloads, http/https args, script content".to_string(),
        },
        LolBin {
            binary: "wscript.exe / cscript.exe".to_string(),
            mitre_ids: vec!["T1064".into(), "T1059.007".into()],
            description: "Windows Script Host".to_string(),
            typical_abuse: "Execute VBScript/JScript files".to_string(),
            detection: "wscript/cscript with .js/.vbs/.wsf, network, encoded commands".to_string(),
        },
        LolBin {
            binary: "certutil.exe".to_string(),
            mitre_ids: vec!["T1105".into()],
            description: "Certificate utility".to_string(),
            typical_abuse: "Download/decode files (-urlcache -decode)".to_string(),
            detection: "certutil with -urlcache/-decode/-verify, network".to_string(),
        },
        LolBin {
            binary: "bitsadmin.exe".to_string(),
            mitre_ids: vec!["T1197".into()],
            description: "BITS transfer".to_string(),
            typical_abuse: "Download/upload files via BITS service".to_string(),
            detection: "bitsadmin /transfer /download, unusual destinations".to_string(),
        },
        LolBin {
            binary: "powershell.exe / pwsh.exe".to_string(),
            mitre_ids: vec!["T1059.001".into()],
            description: "PowerShell".to_string(),
            typical_abuse: "Script execution, download cradles, reflection".to_string(),
            detection: "EncodedCommand, -nop -w hidden, IEX/Invoke-Expression, AMSI bypass"
                .to_string(),
        },
        LolBin {
            binary: "wmic.exe".to_string(),
            mitre_ids: vec!["T1047".into()],
            description: "WMI command line".to_string(),
            typical_abuse: "Process creation, query, lateral movement".to_string(),
            detection: "wmic process call create, /node:remote, unusual parent".to_string(),
        },
        LolBin {
            binary: "schtasks.exe".to_string(),
            mitre_ids: vec!["T1053.005".into()],
            description: "Task scheduler".to_string(),
            typical_abuse: "Create/run tasks for persistence/execution".to_string(),
            detection: "schtasks /create /run, SYSTEM context, unusual paths".to_string(),
        },
        LolBin {
            binary: "at.exe".to_string(),
            mitre_ids: vec!["T1053.002".into()],
            description: "Legacy scheduler".to_string(),
            typical_abuse: "Schedule commands (deprecated but present)".to_string(),
            detection: "at.exe execution, scheduled commands".to_string(),
        },
        LolBin {
            binary: "sc.exe".to_string(),
            mitre_ids: vec!["T1543.003".into()],
            description: "Service control".to_string(),
            typical_abuse: "Create/start services for persistence/execution".to_string(),
            detection: "sc create binpath= with network/path, start=auto".to_string(),
        },
        LolBin {
            binary: "reg.exe".to_string(),
            mitre_ids: vec!["T1112".into()],
            description: "Registry CLI".to_string(),
            typical_abuse: "Modify Run keys, services, policies".to_string(),
            detection: "reg add HKLM\\...\\Run, HKCU\\...\\Run, unusual values".to_string(),
        },
        LolBin {
            binary: "cmd.exe".to_string(),
            mitre_ids: vec!["T1059.003".into()],
            description: "Command shell".to_string(),
            typical_abuse: "Command execution, batch scripts".to_string(),
            detection: "cmd from unusual parent (Word, Excel, Outlook), /c with suspicious args"
                .to_string(),
        },
        LolBin {
            binary: "ftp.exe".to_string(),
            mitre_ids: vec!["T1105".into()],
            description: "FTP client".to_string(),
            typical_abuse: "File transfer (often plaintext)".to_string(),
            detection: "ftp.exe execution, connections to unusual FTP".to_string(),
        },
        LolBin {
            binary: "tftp.exe".to_string(),
            mitre_ids: vec!["T1105".into()],
            description: "TFTP client".to_string(),
            typical_abuse: "File transfer (no auth)".to_string(),
            detection: "tftp.exe -i GET/PUT, unusual hosts".to_string(),
        },
        LolBin {
            binary: "netsh.exe".to_string(),
            mitre_ids: vec!["T1599.001".into()],
            description: "Network shell".to_string(),
            typical_abuse: "Port proxy, firewall modify, helper DLL".to_string(),
            detection: "netsh add portproxy, set firewall, add helper".to_string(),
        },
        LolBin {
            binary: "diskshadow.exe".to_string(),
            mitre_ids: vec!["T1003.003".into()],
            description: "VSS shadow copy".to_string(),
            typical_abuse: "Create shadow copy for NTDS.dit extraction".to_string(),
            detection: "diskshadow.exe execution, shadow copy creation".to_string(),
        },
        LolBin {
            binary: "vssadmin.exe".to_string(),
            mitre_ids: vec!["T1003.003".into()],
            description: "VSS admin".to_string(),
            typical_abuse: "Create/delete shadow copies".to_string(),
            detection: "vssadmin create shadow, list shadows".to_string(),
        },
        LolBin {
            binary: "wbadmin.exe".to_string(),
            mitre_ids: vec!["T1003.003".into()],
            description: "Backup admin".to_string(),
            typical_abuse: "Backup for credential extraction".to_string(),
            detection: "wbadmin start backup, unusual targets".to_string(),
        },
        LolBin {
            binary: "comsvcs.dll".to_string(),
            mitre_ids: vec!["T1003.001".into()],
            description: "COM+ Services DLL".to_string(),
            typical_abuse: "MiniDump via rundll32 (MiniDumpW)".to_string(),
            detection: "rundll32 comsvcs.dll, MiniDump, lsass.dmp".to_string(),
        },
        LolBin {
            binary: "sqldumper.exe".to_string(),
            mitre_ids: vec!["T1003.001".into()],
            description: "SQL Server dumper".to_string(),
            typical_abuse: "Dump LSASS memory".to_string(),
            detection: "sqldumper.exe with PID of lsass, output .dmp".to_string(),
        },
        LolBin {
            binary: "procdump.exe".to_string(),
            mitre_ids: vec!["T1003.001".into()],
            description: "Sysinternals ProcDump".to_string(),
            typical_abuse: "Dump process memory (LSASS)".to_string(),
            detection: "procdump -ma lsass.exe, .dmp creation".to_string(),
        },
        LolBin {
            binary: "makecab.exe".to_string(),
            mitre_ids: vec!["T1560.001".into()],
            description: "Cabinet maker".to_string(),
            typical_abuse: "Compress exfiltration data".to_string(),
            detection: "makecab with unusual files, large cabinets".to_string(),
        },
        LolBin {
            binary: "expand.exe".to_string(),
            mitre_ids: vec!["T1105".into()],
            description: "Cabinet extractor".to_string(),
            typical_abuse: "Extract payloads".to_string(),
            detection: "expand with unusual sources, temp destinations".to_string(),
        },
        LolBin {
            binary: "findstr.exe".to_string(),
            mitre_ids: vec!["T1005".into()],
            description: "String search".to_string(),
            typical_abuse: "Search for credentials in files".to_string(),
            detection: "findstr with password/credential patterns".to_string(),
        },
        LolBin {
            binary: "wusa.exe".to_string(),
            mitre_ids: vec!["T1197".into()],
            description: "Windows Update installer".to_string(),
            typical_abuse: "Extract/install MSU packages".to_string(),
            detection: "wusa with /extract, unusual .msu".to_string(),
        },
        LolBin {
            binary: "msiexec.exe".to_string(),
            mitre_ids: vec!["T1218.007".into()],
            description: "Windows Installer".to_string(),
            typical_abuse: "Execute MSI packages (remote/local)".to_string(),
            detection: "msiexec /i http://, /q, unusual .msi".to_string(),
        },
        LolBin {
            binary: "ieexec.exe".to_string(),
            mitre_ids: vec!["T1218.002".into()],
            description: "IE Exec (legacy)".to_string(),
            typical_abuse: "Execute assemblies via IE".to_string(),
            detection: "ieexec.exe execution (rare, legacy)".to_string(),
        },
        LolBin {
            binary: "control.exe".to_string(),
            mitre_ids: vec!["T1218.004".into()],
            description: "Control Panel".to_string(),
            typical_abuse: "Execute .cpl files".to_string(),
            detection: "control.exe with .cpl argument, unusual path".to_string(),
        },
        LolBin {
            binary: "infdefaultinstall.exe".to_string(),
            mitre_ids: vec!["T1218.009".into()],
            description: "INF installer".to_string(),
            typical_abuse: "Execute INF DefaultInstall section".to_string(),
            detection: "infdefaultinstall with unusual .inf".to_string(),
        },
        LolBin {
            binary: "pcwrun.exe".to_string(),
            mitre_ids: vec!["T1218.012".into()],
            description: "Program Compatibility Wizard".to_string(),
            typical_abuse: "Execute via compatibility shim".to_string(),
            detection: "pcwrun.exe execution".to_string(),
        },
        LolBin {
            binary: "presentationhost.exe".to_string(),
            mitre_ids: vec!["T1218.006".into()],
            description: "XBAP host".to_string(),
            typical_abuse: "Host XAML browser apps".to_string(),
            detection: "presentationhost with .xbap, network".to_string(),
        },
        LolBin {
            binary: "syncappvpublishingserver.exe".to_string(),
            mitre_ids: vec!["T1218.013".into()],
            description: "App-V sync".to_string(),
            typical_abuse: "Execute via App-V".to_string(),
            detection: "syncappvpublishingserver with unusual args".to_string(),
        },
        LolBin {
            binary: "appvlp.exe".to_string(),
            mitre_ids: vec!["T1218.013".into()],
            description: "App-V link".to_string(),
            typical_abuse: "Execute via App-V".to_string(),
            detection: "appvlp.exe execution".to_string(),
        },
    ];

    let mut report = ModuleReport::new(
        "advanced_evasion",
        "lolbins",
        "catalog",
        format!("{} LOLBin/LOLScript entries", bins.len()),
    );

    for b in &bins {
        report = report.line(format!("{} [{}]", b.binary, b.mitre_ids.join(",")));
        report = report.line(format!("  Description: {}", b.description));
        report = report.line(format!("  Typical Abuse: {}", b.typical_abuse));
        report = report.line(format!("  Detection: {}", b.detection));
        report = report.line("");
    }

    report = report.finding(
        Finding::new(
            "advanced_evasion",
            "lolbins",
            "catalog",
            "LOLBin/LOLScript catalog with MITRE ATT&CK mapping",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail("Use for AppLocker/WDAC policy creation, Sysmon rule tuning, and threat hunting"),
    );

    report
}

/// Comprehensive evasion sweep (catalogs only).
pub fn full_sweep() -> ModuleReport {
    let mut combined = ModuleReport::new("advanced_evasion", "sweep", "catalog", String::new());

    let waf = waf_bypass_catalog();
    combined
        .lines
        .push(format!("=== WAF Bypass ({}) ===", waf.summary));
    combined.lines.extend(waf.lines);
    combined.findings.extend(waf.findings);

    let edr = edr_evasion_catalog();
    combined
        .lines
        .push(format!("=== EDR Evasion ({}) ===", edr.summary));
    combined.lines.extend(edr.lines);
    combined.findings.extend(edr.findings);

    let lol = lolbin_catalog();
    combined
        .lines
        .push(format!("=== LOLBins ({}) ===", lol.summary));
    combined.lines.extend(lol.lines);
    combined.findings.extend(lol.findings);

    combined.summary = "completed evasion technique catalogs".to_string();
    combined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waf_bypass_catalog_has_entries() {
        let report = waf_bypass_catalog();
        assert!(report.lines.iter().any(|l| l.contains("SQL Injection")));
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("Cross-Site Scripting")));
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("Remote Code Execution")));
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn edr_evasion_catalog_has_telemetry() {
        let report = edr_evasion_catalog();
        assert!(report.lines.iter().any(|l| l.contains("Direct Syscalls")));
        assert!(report.lines.iter().any(|l| l.contains("DLL Unhooking")));
        assert!(report.lines.iter().any(|l| l.contains("ETW Tampering")));
        assert!(report.lines.iter().any(|l| l.contains("Telemetry Sources")));
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn lolbin_catalog_has_mitre_mapping() {
        let report = lolbin_catalog();
        assert!(report.lines.iter().any(|l| l.contains("msbuild.exe")));
        assert!(report.lines.iter().any(|l| l.contains("T1127.001")));
        assert!(report.lines.iter().any(|l| l.contains("regsvr32")));
        assert!(report.lines.iter().any(|l| l.contains("T1218.010")));
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn full_sweep_combines_all() {
        let report = full_sweep();
        assert!(report.lines.iter().any(|l| l.contains("WAF Bypass")));
        assert!(report.lines.iter().any(|l| l.contains("EDR Evasion")));
        assert!(report.lines.iter().any(|l| l.contains("LOLBins")));
        assert!(report.findings.len() >= 3);
    }
}
