//! Operation coordinator: the single path a HACKER MODE module operation takes.
//!
//! Order is deliberate and non-negotiable:
//! 1. look the operation up in the static table (unknown ops never execute);
//! 2. authorize the target against the assessment *before* any I/O;
//! 3. for anything that connects/fetches, resolve and re-check every address
//!    (a permitted hostname must not become an SSRF into internal space);
//! 4. execute the module;
//! 5. audit allow/deny and duration.
//!
//! Modules never touch the network on their own authority; they receive
//! injected I/O traits from [`Deps`] and are only reachable through [`run`].

use std::net::IpAddr;
use std::time::{Instant, SystemTime};

use super::audit::{AuditLog, AuditRecord};
use super::authorization::{authorize, Decision, Operation};
use super::context::AssessmentContext;
use super::modules::advanced_cloud;
use super::modules::advanced_evasion;
use super::modules::advanced_network;
use super::modules::advanced_recon;
use super::modules::advanced_web;
use super::modules::attack_path;
use super::modules::c2_simulation;
use super::modules::kali::{self, BinaryProbe, PathProbe, ProcessRunner, SystemRunner};
use super::modules::network::{Connector, TcpConnector};
use super::modules::post_exploitation;
use super::modules::recon::{Resolver, SystemResolver};
use super::modules::supply_chain;
use super::modules::vuln::{self, VulnEntry};
use super::modules::web::{Fetcher, ReqwestFetcher};
use super::modules::{exploitation, network, recon, reporting, web, ModuleReport};
use super::risk::{effective_risk, OperationCategory, RiskLevel};
use super::scope::{
    classify_ip, host_in_scope, ip_in_scope, normalize_host, resolved_in_scope, IpClass, ScopeMatch,
};
use super::store::{FindingsBackend, FindingsStore, NullSink, ReportFormat};

/// One executable operation and the policy it is judged under.
#[derive(Debug, Clone, Copy)]
pub struct OpSpec {
    pub module: &'static str,
    pub op: &'static str,
    pub category: OperationCategory,
    /// A target is mandatory (active discovery and above).
    pub needs_target: bool,
    /// The operation connects/fetches, so every resolved address is re-checked.
    pub connects: bool,
    /// Label for a second operand (UI/docs only); empty when there is none.
    pub operand: &'static str,
}

const fn spec(
    module: &'static str,
    op: &'static str,
    category: OperationCategory,
    needs_target: bool,
    connects: bool,
    operand: &'static str,
) -> OpSpec {
    OpSpec {
        module,
        op,
        category,
        needs_target,
        connects,
        operand,
    }
}

/// The static operation table. Adding a module means adding entries here and
/// a `match` arm in [`execute`]; nothing can run outside it.
pub const OPERATIONS: &[OpSpec] = &[
    spec("recon", "dns", OperationCategory::Passive, true, false, ""),
    spec(
        "network",
        "ports",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "",
    ),
    spec(
        "web",
        "headers",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "",
    ),
    spec(
        "vuln",
        "correlate",
        OperationCategory::Passive,
        false,
        false,
        "<component> <version>",
    ),
    spec(
        "exploitation",
        "plan",
        OperationCategory::Intrusive,
        true,
        false,
        "<weakness-class>",
    ),
    spec(
        "kali",
        "available",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "kali",
        "run",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<tool>",
    ),
    spec(
        "reporting",
        "summary",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "reporting",
        "export",
        OperationCategory::Passive,
        false,
        false,
        "<md|json|sarif>",
    ),
    // Advanced Recon
    spec(
        "advanced_recon",
        "ct",
        OperationCategory::Passive,
        true,
        true,
        "<domain>",
    ),
    spec(
        "advanced_recon",
        "asn",
        OperationCategory::Passive,
        true,
        true,
        "<ip|cidr>",
    ),
    spec(
        "advanced_recon",
        "cloud",
        OperationCategory::Passive,
        true,
        true,
        "<domain>",
    ),
    spec(
        "advanced_recon",
        "sweep",
        OperationCategory::Passive,
        true,
        true,
        "<domain>",
    ),
    // Advanced Web
    spec(
        "advanced_web",
        "graphql",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<url>",
    ),
    spec(
        "advanced_web",
        "openapi",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<url>",
    ),
    spec(
        "advanced_web",
        "jwt",
        OperationCategory::Passive,
        false,
        false,
        "<token>",
    ),
    spec(
        "advanced_web",
        "ssrf",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<url> <param>",
    ),
    spec(
        "advanced_web",
        "sweep",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<url>",
    ),
    // Advanced Network
    spec(
        "advanced_network",
        "smb",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<host> <ips...>",
    ),
    spec(
        "advanced_network",
        "kerberos",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<domain> <dc_ips...>",
    ),
    spec(
        "advanced_network",
        "adcs",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<domain> <dc_ips...>",
    ),
    spec(
        "advanced_network",
        "ldap",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<domain> <dc_ips...>",
    ),
    spec(
        "advanced_network",
        "rpc",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<host> <ips...>",
    ),
    spec(
        "advanced_network",
        "sweep",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<target> <ips...>",
    ),
    // Advanced Cloud
    spec(
        "advanced_cloud",
        "aws-metadata",
        OperationCategory::ActiveDiscovery,
        false,
        true,
        "",
    ),
    spec(
        "advanced_cloud",
        "gcp-metadata",
        OperationCategory::ActiveDiscovery,
        false,
        true,
        "",
    ),
    spec(
        "advanced_cloud",
        "azure-metadata",
        OperationCategory::ActiveDiscovery,
        false,
        true,
        "",
    ),
    spec(
        "advanced_cloud",
        "k8s-enum",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<api_server> [token]",
    ),
    spec(
        "advanced_cloud",
        "container-escape",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "advanced_cloud",
        "sweep",
        OperationCategory::ActiveDiscovery,
        true,
        true,
        "<target>",
    ),
    // Advanced Evasion
    spec(
        "advanced_evasion",
        "waf-bypass",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "advanced_evasion",
        "edr-evasion",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "advanced_evasion",
        "lolbins",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "advanced_evasion",
        "sweep",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    // Post Exploitation
    spec(
        "post_exploitation",
        "credential-access",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "post_exploitation",
        "lateral-movement",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "post_exploitation",
        "persistence",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "post_exploitation",
        "sweep",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    // Attack Path
    spec(
        "attack_path",
        "analyze",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "attack_path",
        "export-graphml",
        OperationCategory::Passive,
        false,
        false,
        "<path>",
    ),
    // Supply Chain
    spec(
        "supply_chain",
        "sbom",
        OperationCategory::Passive,
        true,
        false,
        "<path>",
    ),
    spec(
        "supply_chain",
        "dep-confusion",
        OperationCategory::Passive,
        false,
        false,
        "<names> <ecosystems>",
    ),
    spec(
        "supply_chain",
        "repo-jacking",
        OperationCategory::Passive,
        false,
        false,
        "<repos...>",
    ),
    spec(
        "supply_chain",
        "sweep",
        OperationCategory::Passive,
        true,
        false,
        "<sbom_path> <names> <ecosystems> <repos...>",
    ),
    // C2 Simulation
    spec(
        "c2_simulation",
        "profiles",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
    spec(
        "c2_simulation",
        "analyze-traffic",
        OperationCategory::Passive,
        false,
        false,
        "<samples_json> <profiles_json>",
    ),
    spec(
        "c2_simulation",
        "detection-rules",
        OperationCategory::Passive,
        false,
        false,
        "<profile_name>",
    ),
    spec(
        "c2_simulation",
        "simulate",
        OperationCategory::Passive,
        false,
        false,
        "<profile_name> [count]",
    ),
    spec(
        "c2_simulation",
        "sweep",
        OperationCategory::Passive,
        false,
        false,
        "",
    ),
];

/// Looks up an operation (case-insensitive on module and op).
pub fn operation_spec(module: &str, op: &str) -> Option<&'static OpSpec> {
    OPERATIONS.iter().find(|s| {
        s.module.eq_ignore_ascii_case(module.trim()) && s.op.eq_ignore_ascii_case(op.trim())
    })
}

/// Injected I/O for one run. Tests substitute stubs; production uses
/// [`OwnedDeps`].
pub struct Deps<'a> {
    pub resolver: &'a dyn Resolver,
    pub connector: &'a dyn Connector,
    pub fetcher: &'a dyn Fetcher,
    pub vulndb: &'a [VulnEntry],
    /// PATH probe for the Kali bridge.
    pub probe: &'a dyn BinaryProbe,
    /// Bounded process runner for the Kali bridge.
    pub process: &'a dyn ProcessRunner,
    /// Persistence **and** read/export for findings (the `reporting` ops read it).
    pub store: &'a dyn FindingsBackend,
    /// Where `reporting/export` writes; production is `<hacker dir>/reports`.
    pub reports_dir: &'a std::path::Path,
}

/// A sink used when no findings store is configured; module-level, so a
/// reference to it can outlive `OwnedDeps`.
static NULL_SINK: NullSink = NullSink;

/// Production dependencies, owned so they can be moved into `spawn_blocking`.
pub struct OwnedDeps {
    pub resolver: SystemResolver,
    pub connector: TcpConnector,
    pub fetcher: ReqwestFetcher,
    pub vulndb: Vec<VulnEntry>,
    pub probe: PathProbe,
    pub process: SystemRunner,
    /// `None` when the findings store is disabled or failed to open.
    pub store: Option<FindingsStore>,
    /// Output directory for `reporting/export`.
    pub reports_dir: std::path::PathBuf,
}

impl OwnedDeps {
    /// Builds production deps and loads the vulnerability database from
    /// `NANOMITE_HACKER_VULNDB`, else `<hacker_dir>/vulndb.json` when present.
    /// A missing/unreadable database is an empty list (reported honestly by
    /// the `vuln` module), never a silent "no vulnerabilities".
    ///
    /// `store_path` is where findings are persisted; `None` disables
    /// persistence (the `[hacker] store = false` switch). A store that fails to
    /// open degrades to no persistence with a single stderr note, never an
    /// error that blocks the operation.
    pub fn production(store_path: Option<std::path::PathBuf>) -> Self {
        let store = store_path.and_then(|path| match FindingsStore::open(&path) {
            Ok(store) => Some(store),
            Err(err) => {
                eprintln!("hacker: findings store disabled: {err}");
                None
            }
        });
        let mut vulndb = Vec::new();
        let path = std::env::var_os("NANOMITE_HACKER_VULNDB")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| crate::hacker::audit::hacker_dir().join("vulndb.json"));
        if path.is_file() {
            match vuln::load_db(&path) {
                Ok(entries) => vulndb = entries,
                Err(err) => eprintln!("hacker: ignoring vulnerability db: {err}"),
            }
        }
        Self {
            resolver: SystemResolver,
            connector: TcpConnector,
            fetcher: ReqwestFetcher,
            vulndb,
            probe: PathProbe,
            process: SystemRunner,
            store,
            reports_dir: super::store::reports_dir(),
        }
    }

    pub fn deps(&self) -> Deps<'_> {
        let store: &dyn FindingsBackend = match self.store.as_ref() {
            Some(store) => store,
            None => &NULL_SINK,
        };
        Deps {
            resolver: &self.resolver,
            connector: &self.connector,
            fetcher: &self.fetcher,
            vulndb: &self.vulndb,
            probe: &self.probe,
            process: &self.process,
            store,
            reports_dir: &self.reports_dir,
        }
    }
}

/// Runs one authorized operation. `arg` carries the second operand where an
/// operation needs one (`vuln/correlate` = version, `exploitation/plan` =
/// weakness class).
#[allow(clippy::too_many_arguments)]
pub fn run(
    ctx: Option<&AssessmentContext>,
    audit: &AuditLog,
    deps: &Deps<'_>,
    module: &str,
    op: &str,
    target: &str,
    arg: &str,
    now: SystemTime,
) -> Result<ModuleReport, String> {
    let spec = operation_spec(module, op)
        .ok_or_else(|| format!("unknown hacker operation `{}/{}`", module.trim(), op.trim()))?;
    let target = target.trim();
    let started = Instant::now();

    // --- 1. host-level authorization, before any I/O -----------------------
    let host = if spec.needs_target {
        let host = extract_host(target);
        if host.is_empty() {
            let reason = format!("hacker_target_required: `{}` needs a target", spec.module);
            deny(
                audit,
                ctx,
                spec,
                target,
                "hacker_target_required",
                &reason,
                started,
            );
            return Err(reason);
        }
        Some(host)
    } else {
        None
    };

    if let Some(host) = &host {
        // Argv is built without a shell, but a host that starts with `-` or
        // carries whitespace would still be parsed as a flag/extra argument by
        // the target tool, so refuse it outright.
        if spec.module == "kali" && spec.op == "run" && !kali::safe_target(host) {
            let reason = format!("hacker_target_invalid: `{host}` is not a safe host");
            deny(
                audit,
                ctx,
                spec,
                target,
                "hacker_target_invalid",
                &reason,
                started,
            );
            return Err(reason);
        }
        let host_op = Operation::new(spec.module, spec.category).target(host);
        if let Decision::Denied { code, reason } = authorize(ctx, &host_op, now) {
            deny(audit, ctx, spec, target, code, &reason, started);
            return Err(format!("{code}: {reason}"));
        }
    } else {
        let op = Operation::new(spec.module, spec.category);
        if let Decision::Denied { code, reason } = authorize(ctx, &op, now) {
            deny(audit, ctx, spec, target, code, &reason, started);
            return Err(format!("{code}: {reason}"));
        }
    }

    // The assessment must exist past this point; `authorize` proved it.
    let ctx = ctx.expect("authorize allows only with a context");

    // --- 2. resolution + address re-check for anything that connects -------
    let mut resolved: Vec<IpAddr> = Vec::new();
    if let Some(host) = &host {
        if spec.connects {
            match deps.resolver.resolve(host) {
                Ok(ips) => resolved = ips,
                Err(err) => {
                    fail(audit, ctx, spec, target, &err, started);
                    return Err(err);
                }
            }
            if let Err((code, reason)) = resolved_policy(host, &resolved, ctx) {
                deny(audit, Some(ctx), spec, target, code, &reason, started);
                return Err(format!("{code}: {reason}"));
            }
        }
    }

    // --- 3. execute --------------------------------------------------------
    let result = match (spec.module, spec.op) {
        ("recon", "dns") => {
            let host = host.as_deref().unwrap_or(target);
            match deps.resolver.resolve(host) {
                Ok(ips) => {
                    let mut report = recon::dns_with(host, &ips);
                    annotate_out_of_scope(&mut report, &ips, ctx);
                    Ok(report)
                }
                Err(err) => Err(err),
            }
        }
        ("network", "ports") => {
            let host = host.as_deref().unwrap_or(target);
            network::scan_ports(host, &resolved, &network::default_ports(), deps.connector)
        }
        ("web", "headers") => {
            let host = host.as_deref().unwrap_or(target);
            let url = normalize_url(target, host);
            match deps.fetcher.fetch(&url) {
                Ok(fetched) => Ok(web::analyze_headers(&url, &fetched)),
                Err(err) => Err(err),
            }
        }
        ("vuln", "correlate") => Ok(vuln::correlate(target, arg, deps.vulndb)),
        ("exploitation", "plan") => {
            let class = exploitation::WeaknessClass::parse(arg).ok_or_else(|| {
                "exploitation/plan needs a weakness class (e.g. exposed-service)".to_string()
            })?;
            exploitation::plan(target, class)
        }
        ("reporting", "summary") => deps
            .store
            .summary(Some(&ctx.id))
            .map(|summary| reporting::summary_report(&ctx.id, &summary)),
        ("reporting", "export") => {
            let format = ReportFormat::parse(arg)
                .ok_or_else(|| "reporting/export needs a format (md, json or sarif)".to_string())?;
            deps.store
                .export(deps.reports_dir, Some(&ctx.id), format)
                .map(|path| reporting::export_report(&ctx.id, format, &path))
        }
        ("kali", "available") => Ok(kali::available(deps.probe)),
        ("kali", "run") => {
            let host = host.as_deref().unwrap_or(target);
            match kali::find(arg) {
                None => Err(format!(
                    "unknown Kali tool `{arg}` - run `kali/available` for the catalogue"
                )),
                Some(tool) if !deps.probe.present(tool.name) => Err(format!(
                    "`{}` is not installed on PATH (install it, e.g. `sudo apt install {}`)",
                    tool.name, tool.name
                )),
                Some(tool) => {
                    let argv = kali::build_argv(tool, host);
                    let out = deps.process.run(tool.name, &argv, kali::KALI_TIMEOUT)?;
                    Ok(kali::run_report(tool, host, &argv, &out))
                }
            }
        }
        // Advanced Recon
        ("advanced_recon", "ct") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_recon::certificate_transparency(host, &advanced_recon::ReqwestJsonFetcher)
        }
        ("advanced_recon", "asn") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_recon::asn_mapping(host, &advanced_recon::ReqwestJsonFetcher)
        }
        ("advanced_recon", "cloud") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_recon::cloud_asset_enum(
                host,
                &advanced_recon::ReqwestJsonFetcher,
                &advanced_recon::SystemResolver,
            )
        }
        ("advanced_recon", "sweep") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_recon::full_sweep(
                host,
                &advanced_recon::ReqwestJsonFetcher,
                &advanced_recon::SystemResolver,
            )
        }
        // Advanced Web
        ("advanced_web", "graphql") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_web::graphql_introspect(host, &advanced_web::ReqwestFetcher)
        }
        ("advanced_web", "openapi") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_web::openapi_analyze(host, &advanced_web::ReqwestFetcher)
        }
        ("advanced_web", "jwt") => advanced_web::jwt_analyze(arg),
        ("advanced_web", "ssrf") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() < 2 {
                return Err("advanced_web/ssrf needs <url> <param>".into());
            }
            advanced_web::ssrf_probe(parts[0], parts[1], &advanced_web::ReqwestFetcher)
        }
        ("advanced_web", "sweep") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_web::full_sweep(host, &advanced_web::ReqwestFetcher)
        }
        // Advanced Network
        ("advanced_network", "smb") => {
            let host = host.as_deref().unwrap_or(target);
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.is_empty() {
                return Err("advanced_network/smb needs <host> <ips...>".into());
            }
            let ips: Result<Vec<IpAddr>, _> = parts.iter().map(|s| s.parse()).collect();
            match ips {
                Ok(ips) => advanced_network::smb_enum(host, &ips, &advanced_network::TcpConnector),
                Err(_) => Err("invalid IP address".into()),
            }
        }
        ("advanced_network", "kerberos") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() < 2 {
                return Err("advanced_network/kerberos needs <domain> <dc_ips...>".into());
            }
            let domain = parts[0];
            let ips: Result<Vec<IpAddr>, _> = parts[1..].iter().map(|s| s.parse()).collect();
            match ips {
                Ok(ips) => {
                    advanced_network::kerberos_enum(domain, &ips, &advanced_network::TcpConnector)
                }
                Err(_) => Err("invalid IP address".into()),
            }
        }
        ("advanced_network", "adcs") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() < 2 {
                return Err("advanced_network/adcs needs <domain> <dc_ips...>".into());
            }
            let domain = parts[0];
            let ips: Result<Vec<IpAddr>, _> = parts[1..].iter().map(|s| s.parse()).collect();
            match ips {
                Ok(ips) => {
                    advanced_network::adcs_enum(domain, &ips, &advanced_network::TcpConnector)
                }
                Err(_) => Err("invalid IP address".into()),
            }
        }
        ("advanced_network", "ldap") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.len() < 2 {
                return Err("advanced_network/ldap needs <domain> <dc_ips...>".into());
            }
            let domain = parts[0];
            let ips: Result<Vec<IpAddr>, _> = parts[1..].iter().map(|s| s.parse()).collect();
            match ips {
                Ok(ips) => {
                    advanced_network::ldap_enum(domain, &ips, &advanced_network::TcpConnector)
                }
                Err(_) => Err("invalid IP address".into()),
            }
        }
        ("advanced_network", "rpc") => {
            let host = host.as_deref().unwrap_or(target);
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.is_empty() {
                return Err("advanced_network/rpc needs <host> <ips...>".into());
            }
            let ips: Result<Vec<IpAddr>, _> = parts.iter().map(|s| s.parse()).collect();
            match ips {
                Ok(ips) => advanced_network::rpc_enum(host, &ips, &advanced_network::TcpConnector),
                Err(_) => Err("invalid IP address".into()),
            }
        }
        ("advanced_network", "sweep") => {
            let host = host.as_deref().unwrap_or(target);
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.is_empty() {
                return Err("advanced_network/sweep needs <target> <ips...>".into());
            }
            let ips: Result<Vec<IpAddr>, _> = parts.iter().map(|s| s.parse()).collect();
            match ips {
                Ok(ips) => {
                    advanced_network::full_sweep(host, &ips, &advanced_network::TcpConnector)
                }
                Err(_) => Err("invalid IP address".into()),
            }
        }
        // Advanced Cloud
        ("advanced_cloud", "aws-metadata") => {
            advanced_cloud::aws_metadata_check(&advanced_cloud::ReqwestFetcher)
        }
        ("advanced_cloud", "gcp-metadata") => {
            advanced_cloud::gcp_metadata_check(&advanced_cloud::ReqwestFetcher)
        }
        ("advanced_cloud", "azure-metadata") => {
            advanced_cloud::azure_metadata_check(&advanced_cloud::ReqwestFetcher)
        }
        ("advanced_cloud", "k8s-enum") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if parts.is_empty() {
                return Err("advanced_cloud/k8s-enum needs <api_server> [token]".into());
            }
            let token = if parts.len() > 1 {
                Some(parts[1])
            } else {
                None
            };
            advanced_cloud::k8s_api_enum(parts[0], token, &advanced_cloud::ReqwestFetcher)
        }
        ("advanced_cloud", "container-escape") => Ok(advanced_cloud::container_escape_checks()),
        ("advanced_cloud", "sweep") => {
            let host = host.as_deref().unwrap_or(target);
            advanced_cloud::full_sweep(host, &advanced_cloud::ReqwestFetcher)
        }
        // Advanced Evasion
        ("advanced_evasion", "waf-bypass") => Ok(advanced_evasion::waf_bypass_catalog()),
        ("advanced_evasion", "edr-evasion") => Ok(advanced_evasion::edr_evasion_catalog()),
        ("advanced_evasion", "lolbins") => Ok(advanced_evasion::lolbin_catalog()),
        ("advanced_evasion", "sweep") => Ok(advanced_evasion::full_sweep()),
        // Post Exploitation
        ("post_exploitation", "credential-access") => {
            Ok(post_exploitation::credential_access_catalog())
        }
        ("post_exploitation", "lateral-movement") => {
            Ok(post_exploitation::lateral_movement_paths())
        }
        ("post_exploitation", "persistence") => Ok(post_exploitation::persistence_enum()),
        ("post_exploitation", "sweep") => Ok(post_exploitation::full_sweep()),
        // Attack Path
        ("attack_path", "analyze") => attack_path::full_analysis(None, None, None, None),
        ("attack_path", "export-graphml") => {
            use std::path::Path;
            attack_path::export_graphml(&attack_path::AttackGraph::default(), Path::new(arg)).map(
                |_| ModuleReport::new("attack_path", "export-graphml", arg, "graphml exported"),
            )
        }
        // Supply Chain
        ("supply_chain", "sbom") => {
            use std::path::Path;
            supply_chain::sbom_analyze(Path::new(arg))
        }
        ("supply_chain", "dep-confusion") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            if !parts.len().is_multiple_of(2) || parts.is_empty() {
                return Err("supply_chain/dep-confusion needs <names> <ecosystems> pairs".into());
            }
            let mid = parts.len() / 2;
            let names: Vec<String> = parts[..mid].iter().map(|s| s.to_string()).collect();
            let ecosystems: Vec<String> = parts[mid..].iter().map(|s| s.to_string()).collect();
            supply_chain::dependency_confusion_check(&names, &ecosystems)
        }
        ("supply_chain", "repo-jacking") => {
            let repos: Vec<String> = arg.split_whitespace().map(|s| s.to_string()).collect();
            supply_chain::repo_jacking_check(&repos)
        }
        ("supply_chain", "sweep") => {
            // This is a scaffold - needs structured args
            Ok(supply_chain::full_sweep(None, None, None)
                .unwrap_or_else(|e| ModuleReport::new("supply_chain", "sweep", "error", e)))
        }
        // C2 Simulation
        ("c2_simulation", "profiles") => {
            let mut report = ModuleReport::new(
                "c2_simulation",
                "profiles",
                "catalog",
                format!(
                    "{} built-in beacon profiles",
                    c2_simulation::builtin_beacon_profiles().len()
                ),
            );
            for p in c2_simulation::builtin_beacon_profiles() {
                report = report.line(format!("{} ({}) - {}", p.name, p.c2_framework, p.protocol));
            }
            Ok(report)
        }
        ("c2_simulation", "analyze-traffic") => {
            // Expect JSON args - scaffold
            Ok(ModuleReport::new(
                "c2_simulation",
                "analyze-traffic",
                arg,
                "traffic analysis scaffold",
            ))
        }
        ("c2_simulation", "detection-rules") => {
            let profiles = c2_simulation::builtin_beacon_profiles();
            let profile = profiles
                .iter()
                .find(|p| p.name == arg)
                .ok_or_else(|| format!("profile '{}' not found", arg))?;
            Ok(c2_simulation::generate_detection_rules(profile))
        }
        ("c2_simulation", "simulate") => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            let profile_name = parts[0];
            let count = if parts.len() > 1 {
                parts[1].parse().unwrap_or(10)
            } else {
                10
            };
            let profiles = c2_simulation::builtin_beacon_profiles();
            let profile = profiles
                .iter()
                .find(|p| p.name == profile_name)
                .ok_or_else(|| format!("profile '{}' not found", profile_name))?;
            Ok(c2_simulation::simulate_beacon_traffic(profile, count))
        }
        ("c2_simulation", "sweep") => Ok(c2_simulation::full_analysis()),
        _ => Err(format!(
            "operation `{}/{}` is in the table but has no implementation",
            spec.module, spec.op
        )),
    };

    match result {
        Ok(mut report) => {
            // Persist findings before reporting success. A store failure is a
            // note on the report, never a failed operation.
            if deps.store.enabled() {
                if let Err(err) = deps.store.record(ctx, &report) {
                    report
                        .lines
                        .push(format!("note: findings not persisted: {err}"));
                }
            }
            audit.append(
                &AuditRecord::new("allowed", "ok")
                    .with_assessment(ctx.id.clone())
                    .with_module(spec.module)
                    .with_operation(spec.op)
                    .with_target(target)
                    .with_risk(audit_risk(spec, arg).label())
                    .with_duration(started.elapsed()),
            );
            Ok(report)
        }
        Err(err) => {
            fail(audit, ctx, spec, target, &err, started);
            Err(err)
        }
    }
}

/// The stricter-than-`authorize` address rule for operations that connect.
///
/// * A literal/CIDR-scoped target must resolve entirely into allowed CIDRs.
/// * A domain explicitly authorized in `allowed_domains` may resolve to public
///   addresses, but a non-public address (private/link-local/loopback) or the
///   metadata endpoint is refused unless an allowed CIDR literally covers it -
///   the DNS-rebinding / SSRF guard.
pub fn resolved_policy(
    host: &str,
    resolved: &[IpAddr],
    ctx: &AssessmentContext,
) -> Result<(), (&'static str, String)> {
    if resolved.is_empty() {
        return Err((
            "hacker_target_unresolved",
            format!("`{host}` resolved to no addresses"),
        ));
    }
    let is_literal = normalize_host(host)
        .and_then(|h| h.parse::<IpAddr>().ok())
        .is_some();
    let domain_authorized = !is_literal && host_in_scope(host, ctx) == ScopeMatch::InScope;

    if domain_authorized {
        for ip in resolved {
            let external_class = matches!(
                classify_ip(*ip),
                IpClass::Private | IpClass::Loopback | IpClass::LinkLocal | IpClass::Metadata
            );
            if external_class && ip_in_scope(*ip, ctx) != ScopeMatch::InScope {
                return Err((
                    "hacker_resolved_internal",
                    format!(
                        "`{host}` resolves to {ip}, a non-public address not covered by an allowed CIDR"
                    ),
                ));
            }
        }
        Ok(())
    } else if resolved_in_scope(resolved, ctx) == ScopeMatch::InScope {
        Ok(())
    } else {
        Err((
            "hacker_resolved_out_of_scope",
            format!("`{host}` resolves to at least one address outside the authorized scope"),
        ))
    }
}

/// Appends a note for any reported address outside the allowed CIDRs, so a
/// passive DNS result never looks implicitly safe to act on.
fn annotate_out_of_scope(report: &mut ModuleReport, ips: &[IpAddr], ctx: &AssessmentContext) {
    let outside = ips
        .iter()
        .filter(|ip| ip_in_scope(**ip, ctx) != ScopeMatch::InScope)
        .count();
    if outside > 0 {
        report.lines.push(format!(
            "note: {outside} address(es) outside allowed CIDRs - not usable for active ops"
        ));
    }
}

/// Effective risk for audit: the category floor, raised by a Kali tool's
/// declared level when the operation names one.
fn audit_risk(spec: &OpSpec, arg: &str) -> RiskLevel {
    let declared = if spec.module == "kali" && spec.op == "run" {
        kali::find(arg).map(|t| t.risk)
    } else {
        None
    };
    effective_risk(spec.category, declared)
}

fn deny(
    audit: &AuditLog,
    ctx: Option<&AssessmentContext>,
    spec: &OpSpec,
    target: &str,
    code: &str,
    reason: &str,
    started: Instant,
) {
    let mut record = AuditRecord::new("denied", "denied")
        .with_module(spec.module)
        .with_operation(spec.op)
        .with_target(target)
        .with_code(code)
        .with_detail(reason)
        .with_duration(started.elapsed());
    if let Some(ctx) = ctx {
        record = record.with_assessment(ctx.id.clone());
    }
    audit.append(&record);
}

fn fail(
    audit: &AuditLog,
    ctx: &AssessmentContext,
    spec: &OpSpec,
    target: &str,
    err: &str,
    started: Instant,
) {
    audit.append(
        &AuditRecord::new("allowed", "error")
            .with_assessment(ctx.id.clone())
            .with_module(spec.module)
            .with_operation(spec.op)
            .with_target(target)
            .with_detail(err)
            .with_duration(started.elapsed()),
    );
}

/// Extracts a host from a plain host, `host:port`, or a URL. IPv6 literals in
/// brackets are handled; userinfo and paths are dropped.
pub fn extract_host(target: &str) -> String {
    let target = target.trim();
    let without_scheme = target
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(target);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest).to_string();
    }
    authority.split(':').next().unwrap_or(authority).to_string()
}

fn normalize_url(target: &str, host: &str) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        format!("https://{host}")
    }
}

/// Formats the operation list for `/hacker help` and the docs.
pub fn operations_text() -> Vec<String> {
    OPERATIONS
        .iter()
        .map(|s| {
            let operand = if s.operand.is_empty() {
                String::new()
            } else {
                format!(" {}", s.operand)
            };
            let prefix = if s.needs_target { " <target>" } else { "" };
            format!(
                "{}/{} [{}]{operand}{prefix}",
                s.module,
                s.op,
                s.category.label()
            )
        })
        .collect()
}

/// Risk shown in the UI for an operation (category floor).
pub fn operation_risk(spec: &OpSpec) -> RiskLevel {
    spec.category.default_risk()
}

#[cfg(test)]
mod tests {
    use super::super::context::AssessmentStatus;
    use super::super::modules::network::Connector;
    use super::super::store::{FindingSink, ReportSummary};
    use super::*;
    use std::collections::HashSet;
    use std::net::SocketAddr;
    use std::time::Duration;

    struct StubResolver {
        map: Vec<(String, Vec<IpAddr>)>,
    }
    impl Resolver for StubResolver {
        fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
            self.map
                .iter()
                .find(|(h, _)| h == host)
                .map(|(_, ips)| ips.clone())
                .ok_or_else(|| format!("unresolved {host}"))
        }
    }

    struct StubConnector(HashSet<(IpAddr, u16)>);
    impl Connector for StubConnector {
        fn connect(&self, addr: SocketAddr, _t: Duration) -> bool {
            self.0.contains(&(addr.ip(), addr.port()))
        }
    }

    struct StubFetcher(web::Fetched);
    impl Fetcher for StubFetcher {
        fn fetch(&self, _url: &str) -> Result<web::Fetched, String> {
            Ok(self.0.clone())
        }
    }

    struct StubProbe(bool);
    impl BinaryProbe for StubProbe {
        fn present(&self, _name: &str) -> bool {
            self.0
        }
    }

    struct SilentProcess;
    impl ProcessRunner for SilentProcess {
        fn run(
            &self,
            program: &str,
            _args: &[String],
            _timeout: Duration,
        ) -> Result<kali::ProcOutput, String> {
            Ok(kali::ProcOutput {
                exit_code: 0,
                timed_out: false,
                stdout: format!("{program} ran"),
                stderr: String::new(),
                duration_ms: 1,
            })
        }
    }

    /// Records the argv it was given so a test can assert the template.
    struct RecordingProcess(std::sync::Mutex<Vec<String>>);
    impl ProcessRunner for RecordingProcess {
        fn run(
            &self,
            _program: &str,
            args: &[String],
            _timeout: Duration,
        ) -> Result<kali::ProcOutput, String> {
            *self.0.lock().unwrap() = args.to_vec();
            Ok(kali::ProcOutput {
                exit_code: 0,
                timed_out: false,
                stdout: "ok".into(),
                stderr: String::new(),
                duration_ms: 1,
            })
        }
    }

    /// Records what a successful operation asked to persist, without a disk.
    #[derive(Default)]
    struct RecordingSink(std::sync::Mutex<Vec<String>>);
    impl FindingSink for RecordingSink {
        fn record(&self, _ctx: &AssessmentContext, report: &ModuleReport) -> Result<usize, String> {
            let mut seen = self.0.lock().unwrap();
            for f in &report.findings {
                seen.push(format!("{}/{} · {}", f.module, f.operation, f.title));
            }
            Ok(report.findings.len())
        }
    }
    impl FindingsBackend for RecordingSink {
        fn summary(&self, _assessment_id: Option<&str>) -> Result<ReportSummary, String> {
            Ok(ReportSummary {
                total: self.0.lock().unwrap().len(),
                ..ReportSummary::default()
            })
        }
        fn export(
            &self,
            _dir: &std::path::Path,
            _assessment_id: Option<&str>,
            _format: ReportFormat,
        ) -> Result<std::path::PathBuf, String> {
            Err("recording sink does not export".into())
        }
    }

    static PRESENT: StubProbe = StubProbe(true);
    static SILENT: SilentProcess = SilentProcess;
    static NULL_STORE: NullSink = NullSink;

    fn deps<'a>(
        resolver: &'a dyn Resolver,
        connector: &'a dyn Connector,
        fetcher: &'a dyn Fetcher,
        vulndb: &'a [VulnEntry],
    ) -> Deps<'a> {
        Deps {
            resolver,
            connector,
            fetcher,
            vulndb,
            probe: &PRESENT,
            process: &SILENT,
            store: &NULL_STORE,
            reports_dir: std::path::Path::new("."),
        }
    }

    fn ctx() -> AssessmentContext {
        let mut ctx = AssessmentContext::new("asm-1", "Lab");
        ctx.allowed_cidrs = vec!["10.0.0.0/8".into()];
        ctx.allowed_domains = vec!["example.com".into()];
        ctx.permitted_modules = vec![
            "recon".into(),
            "network".into(),
            "web".into(),
            "vuln".into(),
            "exploitation".into(),
            "kali".into(),
            "reporting".into(),
        ];
        ctx.permitted_ops = vec![
            OperationCategory::Passive,
            OperationCategory::ActiveDiscovery,
            OperationCategory::Intrusive,
        ];
        ctx.authorization_ref = Some("LAB-1".into());
        ctx.status = AssessmentStatus::Authorized;
        ctx.start = SystemTime::now();
        ctx.expiry = ctx.start + Duration::from_secs(3600);
        ctx
    }

    fn now(ctx: &AssessmentContext) -> SystemTime {
        ctx.start + Duration::from_secs(10)
    }

    #[test]
    fn table_lookup_is_case_insensitive_and_unknown_is_none() {
        assert!(operation_spec("RECON", "DNS").is_some());
        assert!(operation_spec("recon", "nope").is_none());
        assert!(operation_spec("nope", "dns").is_none());
    }

    #[test]
    fn out_of_scope_target_is_denied_before_io() {
        let ctx = ctx();
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "network",
            "ports",
            "8.8.8.8",
            "",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("hacker_target_out_of_scope"), "{err}");
    }

    #[test]
    fn passive_dns_reports_addresses_and_flags_outside_ones() {
        let ctx = ctx();
        let resolver = StubResolver {
            map: vec![(
                "example.com".into(),
                vec!["10.1.2.3".parse().unwrap(), "8.8.8.8".parse().unwrap()],
            )],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "recon",
            "dns",
            "example.com",
            "",
            now(&ctx),
        )
        .unwrap();
        assert_eq!(report.summary, "2 address(es)");
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("outside allowed CIDRs")));
    }

    #[test]
    fn active_op_refuses_resolved_private_address_outside_scope() {
        let ctx = ctx(); // only 10/8 is allowed; 192.168.0.0/16 is not
        let resolver = StubResolver {
            map: vec![("example.com".into(), vec!["192.168.1.5".parse().unwrap()])],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "network",
            "ports",
            "example.com",
            "",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("hacker_resolved_internal"), "{err}");
    }

    #[test]
    fn active_op_allows_authorized_domain_resolving_public() {
        let ctx = ctx();
        let ip: IpAddr = "93.184.216.34".parse().unwrap();
        let resolver = StubResolver {
            map: vec![("example.com".into(), vec![ip])],
        };
        let connector = StubConnector(HashSet::from([(ip, 443)]));
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "network",
            "ports",
            "example.com",
            "",
            now(&ctx),
        )
        .unwrap();
        assert!(report.summary.contains("1 open"), "{}", report.summary);
    }

    #[test]
    fn web_headers_uses_the_fetcher_and_reports_findings() {
        let ctx = ctx();
        let ip: IpAddr = "10.0.0.9".parse().unwrap();
        let resolver = StubResolver {
            map: vec![("10.0.0.9".into(), vec![ip])],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![("Server".into(), "nginx".into())],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "web",
            "headers",
            "10.0.0.9",
            "",
            now(&ctx),
        )
        .unwrap();
        assert!(report.summary.contains("missing security header"));
    }

    #[test]
    fn vuln_correlate_needs_no_target_and_never_confirms() {
        let ctx = ctx();
        let db = vec![VulnEntry {
            id: "CVE-X".into(),
            component: "widget".into(),
            summary: "bug".into(),
            severity: None,
            at_least: None,
            below: Some("2.0.0".into()),
        }];
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &db);
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "vuln",
            "correlate",
            "widget",
            "1.5.0",
            now(&ctx),
        )
        .unwrap();
        assert!(report.summary.contains("suspected"));
        assert!(report.lines[0].contains("Suspected"));
    }

    #[test]
    fn exploitation_plan_is_gated_and_never_weaponized() {
        let ctx = ctx();
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "exploitation",
            "plan",
            "10.0.0.5",
            "exposed-service",
            now(&ctx),
        )
        .unwrap();
        assert!(report.summary.contains("validation plan"));
        assert!(report.lines.join(" ").contains("Suspected, not Confirmed"));
    }

    #[test]
    fn expired_or_missing_assessment_blocks_everything() {
        let mut ctx = ctx();
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let after = ctx.expiry + Duration::from_secs(1);
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "vuln",
            "correlate",
            "widget",
            "1.0",
            after,
        )
        .unwrap_err();
        assert!(err.contains("hacker_assessment_inactive"), "{err}");

        ctx.status = AssessmentStatus::Revoked;
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "vuln",
            "correlate",
            "widget",
            "1.0",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("hacker_assessment_inactive"), "{err}");

        let err = run(
            None,
            &AuditLog::disabled(),
            &deps,
            "vuln",
            "correlate",
            "widget",
            "1.0",
            SystemTime::now(),
        )
        .unwrap_err();
        assert!(err.contains("hacker_no_assessment"), "{err}");
    }

    #[test]
    fn host_extraction_handles_urls_and_ports() {
        assert_eq!(extract_host("example.com"), "example.com");
        assert_eq!(extract_host("example.com:8443"), "example.com");
        assert_eq!(extract_host("https://example.com/a?b=1"), "example.com");
        assert_eq!(extract_host("https://user:pw@example.com/x"), "example.com");
        assert_eq!(extract_host("http://[::1]:8080/x"), "::1");
    }

    #[test]
    fn successful_operation_persists_its_findings_via_the_sink() {
        let ctx = ctx();
        let ip: IpAddr = "10.0.0.9".parse().unwrap();
        let resolver = StubResolver {
            map: vec![("10.0.0.9".into(), vec![ip])],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![],
        });
        let sink = RecordingSink::default();
        let deps = Deps {
            resolver: &resolver,
            connector: &connector,
            fetcher: &fetcher,
            vulndb: &[],
            probe: &PRESENT,
            process: &SILENT,
            store: &sink,
            reports_dir: std::path::Path::new("."),
        };
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "web",
            "headers",
            "10.0.0.9",
            "",
            now(&ctx),
        )
        .unwrap();
        // The report carries structured findings and the sink saw them.
        assert!(!report.findings.is_empty(), "headers op must emit findings");
        let seen = sink.0.lock().unwrap();
        assert_eq!(seen.len(), report.findings.len());
        assert!(seen
            .iter()
            .any(|s| s.contains("missing content-security-policy")));
    }

    #[test]
    fn reporting_summary_reads_the_store_and_reflects_recorded_findings() {
        let ctx = ctx();
        let ip: IpAddr = "10.0.0.9".parse().unwrap();
        let resolver = StubResolver {
            map: vec![("10.0.0.9".into(), vec![ip])],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 200,
            headers: vec![],
        });
        let store = FindingsStore::open_in_memory().unwrap();
        let deps = Deps {
            resolver: &resolver,
            connector: &connector,
            fetcher: &fetcher,
            vulndb: &[],
            probe: &PRESENT,
            process: &SILENT,
            store: &store,
            reports_dir: std::path::Path::new("."),
        };
        // An empty store summarizes explicitly.
        let empty = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "reporting",
            "summary",
            "",
            "",
            now(&ctx),
        )
        .unwrap();
        assert_eq!(empty.summary, "no findings recorded yet");

        // Record findings, then the summary counts them.
        run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "web",
            "headers",
            "10.0.0.9",
            "",
            now(&ctx),
        )
        .unwrap();
        let summary = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "reporting",
            "summary",
            "",
            "",
            now(&ctx),
        )
        .unwrap();
        assert!(
            summary.summary.ends_with("finding(s)"),
            "{}",
            summary.summary
        );
        assert!(summary.lines.iter().any(|l| l.starts_with("by severity:")));
    }

    #[test]
    fn reporting_export_validates_format_and_writes_a_file() {
        let ctx = ctx();
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let store = FindingsStore::open_in_memory().unwrap();
        let reports = std::env::temp_dir().join(format!("nanomite-reports-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&reports);
        let deps = Deps {
            resolver: &resolver,
            connector: &connector,
            fetcher: &fetcher,
            vulndb: &[],
            probe: &PRESENT,
            process: &SILENT,
            store: &store,
            reports_dir: &reports,
        };
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "reporting",
            "export",
            "",
            "xml",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("needs a format"), "{err}");

        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "reporting",
            "export",
            "",
            "json",
            now(&ctx),
        )
        .unwrap();
        assert!(report.summary.contains("json report written"));
        assert!(report.lines.iter().any(|l| l.contains(".json")));
        let _ = std::fs::remove_dir_all(&reports);
    }

    #[test]
    fn kali_available_lists_the_catalogue() {
        let ctx = ctx();
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "kali",
            "available",
            "",
            "",
            now(&ctx),
        )
        .unwrap();
        assert!(report.summary.contains("installed"), "{}", report.summary);
        assert!(report.lines.iter().any(|l| l.contains("nmap")));
        assert!(report.lines.iter().any(|l| l.contains("nikto")));
    }

    #[test]
    fn kali_run_builds_a_fixed_argv_and_reports_exit() {
        let ctx = ctx();
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        let resolver = StubResolver {
            map: vec![("10.0.0.5".into(), vec![ip])],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let process = RecordingProcess(std::sync::Mutex::new(Vec::new()));
        let deps = Deps {
            resolver: &resolver,
            connector: &connector,
            fetcher: &fetcher,
            vulndb: &[],
            probe: &PRESENT,
            process: &process,
            store: &NULL_STORE,
            reports_dir: std::path::Path::new("."),
        };
        let report = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "kali",
            "run",
            "10.0.0.5",
            "nmap",
            now(&ctx),
        )
        .unwrap();
        let argv = process.0.lock().unwrap().clone();
        assert_eq!(argv.last().unwrap(), "10.0.0.5");
        assert!(argv.contains(&"-sV".to_string()));
        assert_eq!(report.summary, "exit 0 · 1ms");
    }

    #[test]
    fn kali_refuses_flag_like_targets_before_io() {
        let ctx = ctx();
        let resolver = StubResolver { map: vec![] };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let deps = deps(&resolver, &connector, &fetcher, &[]);
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps,
            "kali",
            "run",
            "-oN",
            "nmap",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("hacker_target_invalid"), "{err}");
    }

    #[test]
    fn kali_refuses_uninstalled_and_uncatalogued_tools() {
        let ctx = ctx();
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        let resolver = StubResolver {
            map: vec![("10.0.0.5".into(), vec![ip])],
        };
        let connector = StubConnector(HashSet::new());
        let fetcher = StubFetcher(web::Fetched {
            status: 0,
            headers: vec![],
        });
        let absent = StubProbe(false);
        let deps_absent = Deps {
            resolver: &resolver,
            connector: &connector,
            fetcher: &fetcher,
            vulndb: &[],
            probe: &absent,
            process: &SILENT,
            store: &NULL_STORE,
            reports_dir: std::path::Path::new("."),
        };
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &deps_absent,
            "kali",
            "run",
            "10.0.0.5",
            "nmap",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("not installed"), "{err}");

        let present = deps(&resolver, &connector, &fetcher, &[]);
        let err = run(
            Some(&ctx),
            &AuditLog::disabled(),
            &present,
            "kali",
            "run",
            "10.0.0.5",
            "hydra",
            now(&ctx),
        )
        .unwrap_err();
        assert!(err.contains("unknown Kali tool"), "{err}");
    }
}
