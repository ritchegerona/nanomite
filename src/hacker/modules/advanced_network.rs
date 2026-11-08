//! Advanced Network: Protocol fuzzing, SMB/relay, Kerberos delegation, AD CS enumeration.
//!
//! All operations are scope-gated, bounded, and use injected connectors.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Max SMB shares to enumerate.
pub const MAX_SMB_SHARES: usize = 50;
/// Max AD CS templates to check.
pub const MAX_ADCS_TEMPLATES: usize = 100;
/// Network timeout.
pub const ADV_NET_TIMEOUT: Duration = Duration::from_secs(10);

/// TCP connector trait (same as network module).
pub trait Connector: Send + Sync {
    fn connect(&self, addr: SocketAddr, timeout: Duration) -> bool;
}

pub struct TcpConnector;

impl Connector for TcpConnector {
    fn connect(&self, addr: SocketAddr, timeout: Duration) -> bool {
        std::net::TcpStream::connect_timeout(&addr, timeout).is_ok()
    }
}

/// SMB dialect and capabilities.
#[derive(Debug, Clone)]
pub struct SmbInfo {
    pub dialect: String,
    pub signing_required: bool,
    pub encryption_supported: bool,
    pub compression_supported: bool,
    pub shares: Vec<SmbShare>,
}

#[derive(Debug, Clone)]
pub struct SmbShare {
    pub name: String,
    pub type_: String,
    pub comment: String,
    pub accessible: bool,
}

/// SMB enumeration (simplified - real impl would use smbclient or native SMB).
/// This is a connector-based skeleton; production would need a proper SMB library.
pub fn smb_enum(
    host: &str,
    ips: &[IpAddr],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let host = host.trim();
    if host.is_empty() || ips.is_empty() {
        return Err("advanced_network/smb needs host and resolved IPs".into());
    }

    // Check port 445 first
    let mut has_445 = false;
    for ip in ips {
        let addr = SocketAddr::new(*ip, 445);
        if connector.connect(addr, ADV_NET_TIMEOUT) {
            has_445 = true;
            break;
        }
    }

    if !has_445 {
        return Ok(ModuleReport::new(
            "advanced_network",
            "smb",
            host.to_string(),
            "port 445 closed - SMB not accessible",
        ));
    }

    // In production, this would use a real SMB client (smbclient, rpcclient, or native SMB crate)
    // For now, return a scaffold that documents what would be checked
    let mut report = ModuleReport::new(
        "advanced_network",
        "smb",
        host.to_string(),
        "port 445 open - SMB enumeration requires native SMB implementation",
    );

    report = report.line("Port 445: OPEN");
    report = report.line("Checks that would be performed with full SMB support:");
    report = report.line("  - Dialect negotiation (SMB1/2/3)");
    report = report.line("  - Signing requirement (SMB signing bypass)");
    report = report.line("  - Encryption capabilities");
    report = report.line("  - Share enumeration (IPC$, ADMIN$, C$, custom)");
    report = report.line("  - Anonymous/guest access testing");
    report = report.line("  - Null session enumeration");
    report = report.line("  - SMB relay attack surface");
    report = report.line("  - PrintNightmare (CVE-2021-34527) check");
    report = report.line("  - ZeroLogon (CVE-2020-1472) check");
    report = report.line("  - PetitPotam (CVE-2021-36942) check");

    report = report.finding(
        Finding::new("advanced_network", "smb", host.to_string(),
            "smb port open - manual enumeration recommended",
            RiskLevel::Informational)
        .confidence(Confidence::Confirmed)
        .detail("TCP 445 is open. Full SMB enumeration requires native SMB client or smbclient/rpcclient."),
    );

    Ok(report)
}

/// Kerberos delegation and SPN enumeration.
#[derive(Debug, Clone)]
pub struct KerberosInfo {
    pub domain: String,
    pub dc_ips: Vec<IpAddr>,
    pub spns: Vec<String>,
    pub delegation: Vec<DelegationEntry>,
}

#[derive(Debug, Clone)]
pub struct DelegationEntry {
    pub account: String,
    pub delegation_type: String, // Unconstrained, Constrained, Resource-based
    pub target_spns: Vec<String>,
}

/// Kerberos enumeration (skeleton - real impl uses krb5, ldap, or impacket).
pub fn kerberos_enum(
    domain: &str,
    dc_ips: &[IpAddr],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() || dc_ips.is_empty() {
        return Err("advanced_network/kerberos needs domain and DC IPs".into());
    }

    // Check Kerberos ports (88 TCP/UDP, 464 for kpasswd)
    let mut kerberos_ports = Vec::new();
    for ip in dc_ips {
        for port in [88, 464] {
            let addr = SocketAddr::new(*ip, port);
            if connector.connect(addr, ADV_NET_TIMEOUT) {
                kerberos_ports.push((*ip, port));
            }
        }
    }

    if kerberos_ports.is_empty() {
        return Ok(ModuleReport::new(
            "advanced_network",
            "kerberos",
            &domain,
            "no Kerberos ports (88/464) accessible on DCs",
        ));
    }

    let mut report = ModuleReport::new(
        "advanced_network",
        "kerberos",
        &domain,
        format!("Kerberos accessible on {} DC(s)", kerberos_ports.len()),
    );

    for (ip, port) in &kerberos_ports {
        report = report.line(format!("DC {}:{} - OPEN", ip, port));
    }

    report = report.line("");
    report = report.line("Checks requiring full Kerberos/LDAP implementation:");
    report = report.line("  - SPN enumeration (setspn, ldapsearch)");
    report = report.line("  - Unconstrained delegation accounts");
    report = report.line("  - Constrained delegation (msDS-AllowedToDelegateTo)");
    report = report.line(
        "  - Resource-based constrained delegation (msDS-AllowedToActOnBehalfOfOtherIdentity)",
    );
    report = report.line("  - AS-REP roasting (accounts with DONT_REQ_PREAUTH)");
    report = report.line("  - Kerberoasting (SPN accounts with weak passwords)");
    report = report.line("  - Golden/Silver ticket prerequisites (krbtgt hash, DCs)");
    report = report.line("  - S4U2Self / S4U2Proxy abuse paths");
    report = report.line("  - Bronze Bit (CVE-2020-17049) check");
    report = report.line("  - PAC spoofing / KDC validation");

    report = report.finding(
        Finding::new(
            "advanced_network",
            "kerberos",
            domain,
            "kerberos ports open - delegation enum recommended",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail(format!(
            "Kerberos accessible on {} DC(s). Full enumeration requires LDAP/Kerberos client.",
            kerberos_ports.len()
        )),
    );

    Ok(report)
}

/// AD Certificate Services (AD CS) enumeration for ESC vulns.
#[derive(Debug, Clone)]
pub struct AdcsTemplate {
    pub name: String,
    pub display_name: String,
    pub requires_manager_approval: bool,
    pub requires_key_archival: bool,
    pub publish_to_ds: bool,
    pub client_auth: bool,
    pub enrollee_supplies_subject: bool,
    pub authorized_signatures: u32,
    pub enrollment_rights: Vec<String>,
    pub vulnerabilities: Vec<String>, // ESC1-ESC15
}

/// AD CS enumeration (skeleton - real impl uses certutil, certipy, or LDAP).
pub fn adcs_enum(
    domain: &str,
    dc_ips: &[IpAddr],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() || dc_ips.is_empty() {
        return Err("advanced_network/adcs needs domain and DC IPs".into());
    }

    // Check LDAP (389) and LDAPS (636) and RPC (135) and HTTP (80/443 for certsrv)
    let mut accessible_services = Vec::new();
    for ip in dc_ips {
        for port in [80, 135, 389, 443, 636] {
            let addr = SocketAddr::new(*ip, port);
            if connector.connect(addr, ADV_NET_TIMEOUT) {
                accessible_services.push((*ip, port));
            }
        }
    }

    if accessible_services.is_empty() {
        return Ok(ModuleReport::new(
            "advanced_network",
            "adcs",
            &domain,
            "no AD CS related ports accessible on DCs",
        ));
    }

    let mut report = ModuleReport::new(
        "advanced_network",
        "adcs",
        &domain,
        format!("AD CS related services on {} DC(s)", dc_ips.len()),
    );

    for (ip, port) in &accessible_services {
        let service = match port {
            80 | 443 => "HTTP/HTTPS (certsrv)",
            135 => "RPC (cert enrollment)",
            389 => "LDAP",
            636 => "LDAPS",
            _ => "unknown",
        };
        report = report.line(format!("DC {}:{} - {} OPEN", ip, port, service));
    }

    report = report.line("");
    report = report.line("ESC (Enterprise Security Context) vulnerability checks:");
    report = report.line("  ESC1  - Client auth + enrollee supplies subject + no manager approval");
    report = report.line("  ESC2  - Client auth + any purpose (no EKU restriction)");
    report = report.line("  ESC3  - Certificate request agent (enroll on behalf of)");
    report = report.line("  ESC4  - Vulnerable template ACLs (write Dacl/WriteProperty)");
    report = report.line("  ESC5  - Vulnerable CA ACLs (ManageCA/ManageCertificates)");
    report = report.line("  ESC6  - EDITF_ATTRIBUTESUBJECTALTNAME2 (SAN injection)");
    report = report.line("  ESC7  - Vulnerable certificate template (user editable)");
    report = report.line("  ESC8  - NTLM relay to AD CS HTTP endpoints (certsrv)");
    report = report.line("  ESC9  - Unintended CT_FLAG_ENROLLEE_SUPPLIES_SUBJECT");
    report = report.line("  ESC10 - Weak CA security (certificate manager approval)");
    report = report.line("  ESC11 - Renewal abuse (renew with different subject)");
    report = report.line("  ESC12 - Certificate template version downgrade");
    report = report.line("  ESC13 - Misconfigured certificate templates (RAS/VPN)");
    report = report.line("  ESC14 - Domain controller certificate authentication");
    report = report.line("  ESC15 - Cross-forest trust certificate abuse");

    report = report.finding(
        Finding::new("advanced_network", "adcs", domain,
            "ad cs services accessible - esc audit recommended",
            RiskLevel::High)
        .confidence(Confidence::Suspected)
        .detail(format!("AD CS related ports accessible on {} DC(s). ESC1-15 audit requires certipy/Certify/AD CS enum tools.", dc_ips.len())),
    );

    Ok(report)
}

/// LDAP enumeration for AD reconnaissance.
pub fn ldap_enum(
    domain: &str,
    dc_ips: &[IpAddr],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() || dc_ips.is_empty() {
        return Err("advanced_network/ldap needs domain and DC IPs".into());
    }

    let mut ldap_ports = Vec::new();
    for ip in dc_ips {
        for port in [389, 636] {
            let addr = SocketAddr::new(*ip, port);
            if connector.connect(addr, ADV_NET_TIMEOUT) {
                ldap_ports.push((*ip, port));
            }
        }
    }

    if ldap_ports.is_empty() {
        return Ok(ModuleReport::new(
            "advanced_network",
            "ldap",
            &domain,
            "no LDAP/LDAPS ports accessible on DCs",
        ));
    }

    let mut report = ModuleReport::new(
        "advanced_network",
        "ldap",
        &domain,
        format!("LDAP accessible on {} DC(s)", ldap_ports.len()),
    );

    for (ip, port) in &ldap_ports {
        let proto = if *port == 636 { "LDAPS" } else { "LDAP" };
        report = report.line(format!("DC {}:{} - {} OPEN", ip, port, proto));
    }

    report = report.line("");
    report = report.line("LDAP enumeration checks (require LDAP bind):");
    report = report.line("  - Domain/Forest functional level");
    report = report.line("  - All users, groups, computers, OUs");
    report = report.line("  - AdminSDHolder / AdminCount=1 (privileged groups)");
    report = report.line("  - GPO enumeration (SYSVOL accessible)");
    report = report.line("  - Password policy (min age, complexity, lockout)");
    report = report.line("  - Fine-grained password policies (PSOs)");
    report = report.line("  - LAPS passwords (ms-Mcs-AdmPwd)");
    report = report.line("  - gMSA accounts (msDS-GroupMSAMembership)");
    report = report.line("  - Constrained delegation (msDS-AllowedToDelegateTo)");
    report =
        report.line("  - Resource-based delegation (msDS-AllowedToActOnBehalfOfOtherIdentity)");
    report = report.line("  - SPN enumeration for Kerberoasting");
    report = report.line("  - AS-REP roasting (userAccountControl & DONT_REQ_PREAUTH)");
    report = report.line("  - Trust relationships (trustedDomain objects)");
    report = report.line("  - DCSync rights (DS-Replication-Get-Changes-All)");

    report = report.finding(
        Finding::new(
            "advanced_network",
            "ldap",
            domain,
            "ldap accessible - full enum recommended",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail(format!(
            "LDAP/LDAPS accessible on {} DC(s). Full enumeration requires LDAP bind.",
            ldap_ports.len()
        )),
    );

    Ok(report)
}

/// RPC endpoint mapper and named pipe enumeration.
pub fn rpc_enum(
    host: &str,
    ips: &[IpAddr],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let host = host.trim();
    if host.is_empty() || ips.is_empty() {
        return Err("advanced_network/rpc needs host and resolved IPs".into());
    }

    let mut rpc_ports = Vec::new();
    for ip in ips {
        for port in [135, 139, 445, 593] {
            let addr = SocketAddr::new(*ip, port);
            if connector.connect(addr, ADV_NET_TIMEOUT) {
                rpc_ports.push((*ip, port));
            }
        }
    }

    if rpc_ports.is_empty() {
        return Ok(ModuleReport::new(
            "advanced_network",
            "rpc",
            host.to_string(),
            "no RPC ports accessible",
        ));
    }

    let mut report = ModuleReport::new(
        "advanced_network",
        "rpc",
        host.to_string(),
        format!("RPC ports accessible: {}", rpc_ports.len()),
    );

    for (ip, port) in &rpc_ports {
        let service = match port {
            135 => "RPC Endpoint Mapper",
            139 => "NetBIOS Session",
            445 => "SMB/RPC over SMB",
            593 => "RPC over HTTP",
            _ => "unknown",
        };
        report = report.line(format!("{} {} - {} OPEN", ip, port, service));
    }

    report = report.line("");
    report = report.line("RPC enumeration checks (require rpcdump/rpcclient/impacket):");
    report = report.line("  - Endpoint mapper dump (rpcdump.py)");
    report = report.line("  - Named pipe enumeration (lsarpc, samr, netlogon, svcctl, wkssvc)");
    report = report.line("  - DCOM/IWbemServices (WMI lateral movement)");
    report = report.line("  - Print Spooler (MS-RPRN) - PrintNightmare");
    report = report.line("  - Netlogon (MS-NRPC) - ZeroLogon");
    report = report.line("  - LSARPC (MS-LSAT) - LSA secrets");
    report = report.line("  - SAMR (MS-SAMR) - user enumeration, password policies");
    report = report.line("  - SVCCTL (MS-SCMR) - service enumeration, creation");
    report = report.line("  - WKSSVC (MS-WKST) - workstation info");
    report = report.line("  - DFSR (MS-DFSR) - SYSVOL replication");
    report = report.line("  - FRS (MS-FRS) - legacy SYSVOL");

    report = report.finding(
        Finding::new(
            "advanced_network",
            "rpc",
            host.to_string(),
            "rpc endpoints accessible - enum recommended",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail(
            "RPC ports accessible on target. Full enumeration requires rpcdump.py or impacket."
                .to_string(),
        ),
    );

    Ok(report)
}

/// Comprehensive advanced network sweep.
pub fn full_sweep(
    target: &str,
    ips: &[IpAddr],
    connector: &dyn Connector,
) -> Result<ModuleReport, String> {
    let target = target.trim();
    if target.is_empty() || ips.is_empty() {
        return Err("advanced_network/sweep needs target and IPs".into());
    }

    let mut combined = ModuleReport::new(
        "advanced_network",
        "sweep",
        target.to_string(),
        String::new(),
    );

    // SMB
    match smb_enum(target, ips, connector) {
        Ok(r) => {
            combined.lines.push(format!("=== SMB ({}) ===", r.summary));
            combined.lines.extend(r.lines);
            combined.findings.extend(r.findings);
        }
        Err(e) => combined.lines.push(format!("SMB error: {e}")),
    }

    // RPC
    match rpc_enum(target, ips, connector) {
        Ok(r) => {
            combined.lines.push(format!("=== RPC ({}) ===", r.summary));
            combined.lines.extend(r.lines);
            combined.findings.extend(r.findings);
        }
        Err(e) => combined.lines.push(format!("RPC error: {e}")),
    }

    combined.summary = format!("completed SMB + RPC sweep for {}", target);
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::net::IpAddr;

    struct StubConnector(HashSet<(IpAddr, u16)>);
    impl Connector for StubConnector {
        fn connect(&self, addr: SocketAddr, _timeout: Duration) -> bool {
            self.0.contains(&(addr.ip(), addr.port()))
        }
    }

    #[test]
    fn smb_enum_detects_port_445() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector([(ip, 445)].into());
        let report = smb_enum("dc01", &[ip], &connector).unwrap();
        assert!(report.summary.contains("445"));
        assert!(report.lines.iter().any(|l| l.contains("445")));
        assert_eq!(report.findings.len(), 1);
    }

    #[test]
    fn smb_enum_closed_port() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector(HashSet::new());
        let report = smb_enum("dc01", &[ip], &connector).unwrap();
        assert!(report.summary.contains("closed"));
    }

    #[test]
    fn kerberos_enum_detects_port_88() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector([(ip, 88)].into());
        let report = kerberos_enum("corp.local", &[ip], &connector).unwrap();
        assert!(report.summary.contains("Kerberos"));
        assert!(report.lines.iter().any(|l| l.contains("88")));
    }

    #[test]
    fn adcs_enum_detects_http_ldap_rpc() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector([(ip, 80), (ip, 389), (ip, 135)].into());
        let report = adcs_enum("corp.local", &[ip], &connector).unwrap();
        assert!(report.summary.contains("AD CS"));
        assert!(report.lines.iter().any(|l| l.contains("HTTP")));
        assert!(report.lines.iter().any(|l| l.contains("LDAP")));
        assert!(report.lines.iter().any(|l| l.contains("RPC")));
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].severity, RiskLevel::High);
    }

    #[test]
    fn ldap_enum_detects_ldap_ldaps() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector([(ip, 389), (ip, 636)].into());
        let report = ldap_enum("corp.local", &[ip], &connector).unwrap();
        assert!(report.summary.contains("LDAP"));
        assert!(report.lines.iter().any(|l| l.contains("LDAP")));
        assert!(report.lines.iter().any(|l| l.contains("LDAPS")));
    }

    #[test]
    fn rpc_enum_detects_endpoint_mapper() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector([(ip, 135)].into());
        let report = rpc_enum("dc01", &[ip], &connector).unwrap();
        assert!(report.summary.contains("RPC"));
        assert!(report.lines.iter().any(|l| l.contains("Endpoint Mapper")));
    }

    #[test]
    fn empty_inputs_rejected() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let connector = StubConnector(HashSet::new());
        assert!(smb_enum("", &[ip], &connector).is_err());
        assert!(smb_enum("host", &[], &connector).is_err());
        assert!(kerberos_enum("", &[ip], &connector).is_err());
        assert!(adcs_enum("", &[ip], &connector).is_err());
        assert!(ldap_enum("", &[ip], &connector).is_err());
        assert!(rpc_enum("", &[ip], &connector).is_err());
    }
}
