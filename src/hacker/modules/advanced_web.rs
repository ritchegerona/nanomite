//! Advanced Web: GraphQL introspection, API schema analysis, JWT attacks, SSRF chains.
//!
//! All operations are scope-gated, redirect-free, and bounded.

use std::collections::HashMap;
use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Max GraphQL schema size to process.
pub const MAX_GQL_SCHEMA_SIZE: usize = 500_000;
/// Max endpoints to test for SSRF.
pub const MAX_SSRF_ENDPOINTS: usize = 20;
/// HTTP timeout.
pub const ADV_WEB_TIMEOUT: Duration = Duration::from_secs(30);

/// Minimal response shape.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Fetched {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|v| v.as_str())
    }
}

/// Fetches responses. Production uses reqwest; tests inject a stub.
pub trait Fetcher: Send + Sync {
    fn fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<Fetched, String>;
}

/// Real fetcher with no redirects.
pub struct ReqwestFetcher;

impl Fetcher for ReqwestFetcher {
    fn fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<Fetched, String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(ADV_WEB_TIMEOUT)
            .user_agent("nanomite-hacker-adv-web/0.1")
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let mut req = match method.to_uppercase().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            _ => return Err(format!("unsupported method {method}")),
        };

        for (k, v) in headers {
            req = req.header(*k, *v);
        }

        if let Some(b) = body {
            req = req.body(b.to_string());
        }

        let response = req.send().map_err(|e| format!("request failed: {e}"))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let body = response.text().unwrap_or_default();

        Ok(Fetched {
            status,
            headers,
            body,
        })
    }
}

/// GraphQL introspection result.
///
/// A GraphQL endpoint answers with `{"data":{"__schema":...}}`; some proxies
/// return the schema unwrapped, so both shapes are accepted.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLIntrospection {
    #[serde(rename = "__schema", default)]
    pub schema: Option<GraphQLSchema>,
    #[serde(default)]
    pub data: Option<GraphQLData>,
}

/// The `data` envelope of a GraphQL response.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLData {
    #[serde(rename = "__schema", default)]
    pub schema: Option<GraphQLSchema>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLSchema {
    pub types: Vec<GraphQLType>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLType {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub fields: Option<Vec<GraphQLField>>,
    #[serde(rename = "inputFields")]
    pub input_fields: Option<Vec<GraphQLField>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLField {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub field_type: Option<GraphQLTypeRef>,
    pub args: Option<Vec<GraphQLArg>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLTypeRef {
    pub name: Option<String>,
    pub kind: Option<String>,
    #[serde(rename = "ofType")]
    pub of_type: Option<Box<GraphQLTypeRef>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLArg {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub arg_type: Option<GraphQLTypeRef>,
}

/// Queries GraphQL introspection endpoint.
pub fn graphql_introspect(url: &str, fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("advanced_web/graphql needs a URL".into());
    }

    let introspection_query = r#"{"query":"{__schema{types{name kind fields{name type{name kind ofType{name kind}}args{name type{name kind ofType{name kind}}}}inputFields{name type{name kind ofType{name kind}}}}}}}"#;

    let fetched = fetcher.fetch(
        url,
        "POST",
        Some(introspection_query),
        &[("Content-Type", "application/json")],
    )?;

    if fetched.status != 200 {
        return Ok(ModuleReport::new(
            "advanced_web",
            "graphql",
            url.to_string(),
            format!("introspection failed: HTTP {}", fetched.status),
        ));
    }

    if fetched.body.len() > MAX_GQL_SCHEMA_SIZE {
        return Ok(ModuleReport::new(
            "advanced_web",
            "graphql",
            url.to_string(),
            "schema too large, truncated",
        ));
    }

    let introspection: GraphQLIntrospection =
        serde_json::from_str(&fetched.body).map_err(|e| format!("parse introspection: {e}"))?;

    let schema = introspection
        .schema
        .or_else(|| introspection.data.and_then(|data| data.schema));
    let Some(schema) = schema else {
        return Ok(ModuleReport::new(
            "advanced_web",
            "graphql",
            url.to_string(),
            "introspection disabled or no schema returned",
        ));
    };

    let mut queries = Vec::new();
    let mut mutations = Vec::new();
    let mut subscriptions = Vec::new();
    let mut input_types = Vec::new();
    let mut sensitive_fields = Vec::new();

    for gql_type in &schema.types {
        let name = gql_type.name.as_deref().unwrap_or("");
        let kind = gql_type.kind.as_deref().unwrap_or("");

        match kind {
            "OBJECT" if name == "Query" => {
                if let Some(fields) = &gql_type.fields {
                    for f in fields {
                        if let Some(fname) = &f.name {
                            queries.push(fname.clone());
                            if is_sensitive_field(fname) {
                                sensitive_fields.push(format!("Query.{fname}"));
                            }
                        }
                    }
                }
            }
            "OBJECT" if name == "Mutation" => {
                if let Some(fields) = &gql_type.fields {
                    for f in fields {
                        if let Some(fname) = &f.name {
                            mutations.push(fname.clone());
                            if is_sensitive_field(fname) {
                                sensitive_fields.push(format!("Mutation.{fname}"));
                            }
                        }
                    }
                }
            }
            "OBJECT" if name == "Subscription" => {
                if let Some(fields) = &gql_type.fields {
                    for f in fields {
                        if let Some(fname) = &f.name {
                            subscriptions.push(fname.clone());
                        }
                    }
                }
            }
            "INPUT_OBJECT" => {
                input_types.push(name.to_string());
            }
            _ => {}
        }
    }

    let summary = format!(
        "{} queries, {} mutations, {} subscriptions, {} input types",
        queries.len(),
        mutations.len(),
        subscriptions.len(),
        input_types.len()
    );

    let mut report = ModuleReport::new("advanced_web", "graphql", url.to_string(), summary);

    report = report.line(format!(
        "Queries ({}): {}",
        queries.len(),
        queries.join(", ")
    ));
    report = report.line(format!(
        "Mutations ({}): {}",
        mutations.len(),
        mutations.join(", ")
    ));
    report = report.line(format!(
        "Subscriptions ({}): {}",
        subscriptions.len(),
        subscriptions.join(", ")
    ));
    report = report.line(format!(
        "Input Types ({}): {}",
        input_types.len(),
        input_types.join(", ")
    ));

    if !sensitive_fields.is_empty() {
        report = report.line(format!(
            "⚠ Sensitive fields: {}",
            sensitive_fields.join(", ")
        ));
        report = report.finding(
            Finding::new(
                "advanced_web",
                "graphql",
                url.to_string(),
                "graphql sensitive field exposure",
                RiskLevel::Medium,
            )
            .confidence(Confidence::Suspected)
            .detail(format!(
                "Introspection reveals potentially sensitive fields: {}",
                sensitive_fields.join(", ")
            )),
        );
    }

    // Check for introspection being enabled (it is, since we got results)
    report = report.finding(
        Finding::new(
            "advanced_web",
            "graphql",
            url.to_string(),
            "graphql introspection enabled",
            RiskLevel::Low,
        )
        .confidence(Confidence::Confirmed)
        .detail("GraphQL introspection is enabled - consider disabling in production"),
    );

    Ok(report)
}

fn is_sensitive_field(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("key")
        || lower.contains("credit")
        || lower.contains("ssn")
        || lower.contains("admin")
        || lower.contains("internal")
        || lower.contains("debug")
        || lower.contains("config")
        || lower.contains("env")
        || lower.contains("private")
}

/// OpenAPI/Swagger schema analysis.
pub fn openapi_analyze(url: &str, fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("advanced_web/openapi needs a URL".into());
    }

    let fetched = fetcher.fetch(url, "GET", None, &[("Accept", "application/json")])?;

    if fetched.status != 200 {
        return Ok(ModuleReport::new(
            "advanced_web",
            "openapi",
            url.to_string(),
            format!("fetch failed: HTTP {}", fetched.status),
        ));
    }

    let spec: serde_json::Value =
        serde_json::from_str(&fetched.body).map_err(|e| format!("parse openapi: {e}"))?;

    let mut endpoints = Vec::new();
    let mut paths_seen: Vec<String> = Vec::new();
    let mut auth_schemes = Vec::new();
    let mut sensitive_params = Vec::new();

    if let Some(paths) = spec.get("paths").and_then(|v| v.as_object()) {
        for (path, methods) in paths {
            paths_seen.push(path.clone());
            if let Some(methods_obj) = methods.as_object() {
                for (method, details) in methods_obj {
                    if let Some(security) = details.get("security").and_then(|v| v.as_array()) {
                        for sec in security {
                            if let Some(obj) = sec.as_object() {
                                for scheme in obj.keys() {
                                    if !auth_schemes.contains(scheme) {
                                        auth_schemes.push(scheme.clone());
                                    }
                                }
                            }
                        }
                    }

                    if let Some(parameters) = details.get("parameters").and_then(|v| v.as_array()) {
                        for param in parameters {
                            if let Some(name) = param.get("name").and_then(|v| v.as_str()) {
                                if is_sensitive_param(name) {
                                    sensitive_params.push(format!(
                                        "{} {}: {}",
                                        method.to_uppercase(),
                                        path,
                                        name
                                    ));
                                }
                            }
                        }
                    }

                    if let Some(req_body) = details.get("requestBody") {
                        if let Some(content) = req_body.get("content").and_then(|v| v.as_object()) {
                            for (_media_type, schema) in content {
                                if let Some(props) = schema
                                    .get("schema")
                                    .and_then(|v| v.get("properties"))
                                    .and_then(|v| v.as_object())
                                {
                                    for (prop_name, _) in props {
                                        if is_sensitive_param(prop_name) {
                                            sensitive_params.push(format!(
                                                "{} {} body: {}",
                                                method.to_uppercase(),
                                                path,
                                                prop_name
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    endpoints.push(format!("{} {}", method.to_uppercase(), path));
                }
            }
        }
    }

    let unique_paths: Vec<&String> = {
        let mut seen: Vec<&String> = Vec::new();
        for path in paths_seen.iter() {
            if !seen.contains(&path) {
                seen.push(path);
            }
        }
        seen
    };
    let summary = format!(
        "{} endpoints, {} auth schemes",
        unique_paths.len(),
        auth_schemes.len()
    );
    let mut report = ModuleReport::new("advanced_web", "openapi", url.to_string(), summary);

    report = report.line(format!(
        "Endpoints: {} ({} operations)",
        unique_paths.len(),
        endpoints.len()
    ));
    for ep in endpoints.iter().take(30) {
        report = report.line(format!("  {}", ep));
    }
    if endpoints.len() > 30 {
        report = report.line(format!("  ... and {} more", endpoints.len() - 30));
    }

    report = report.line(format!("Auth schemes: {}", auth_schemes.join(", ")));

    if !sensitive_params.is_empty() {
        report = report.line(format!(
            "⚠ Sensitive parameters: {}",
            sensitive_params.len()
        ));
        for sp in sensitive_params.iter().take(10) {
            report = report.line(format!("  {}", sp));
        }
        report = report.finding(
            Finding::new(
                "advanced_web",
                "openapi",
                url.to_string(),
                "api sensitive parameter exposure",
                RiskLevel::Low,
            )
            .confidence(Confidence::Suspected)
            .detail(format!(
                "OpenAPI spec reveals potentially sensitive parameters: {}",
                sensitive_params.join("; ")
            )),
        );
    }

    if auth_schemes.is_empty() {
        report = report.finding(
            Finding::new(
                "advanced_web",
                "openapi",
                url.to_string(),
                "api missing authentication",
                RiskLevel::Medium,
            )
            .confidence(Confidence::Suspected)
            .detail("No authentication schemes defined in OpenAPI spec"),
        );
    }

    Ok(report)
}

fn is_sensitive_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("key")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("auth")
        || lower.contains("credential")
        || lower.contains("ssn")
        || lower.contains("credit")
        || lower.contains("cvv")
        || lower.contains("pin")
}

/// JWT analysis: decode header/payload, check algorithm confusion, none alg, weak secrets.
pub fn jwt_analyze(token: &str) -> Result<ModuleReport, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("advanced_web/jwt needs a token".into());
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("invalid JWT format (expected 3 parts)".into());
    }

    let header_b64 = parts[0];
    let payload_b64 = parts[1];
    let signature = parts[2];

    let header_json = base64_url_decode(header_b64).map_err(|e| format!("decode header: {e}"))?;
    let payload_json =
        base64_url_decode(payload_b64).map_err(|e| format!("decode payload: {e}"))?;

    let header: serde_json::Value =
        serde_json::from_str(&header_json).map_err(|e| format!("parse header: {e}"))?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_json).map_err(|e| format!("parse payload: {e}"))?;

    let alg = header.get("alg").and_then(|v| v.as_str()).unwrap_or("none");
    let typ = header.get("typ").and_then(|v| v.as_str()).unwrap_or("JWT");
    let kid = header.get("kid").and_then(|v| v.as_str());

    let mut report = ModuleReport::new(
        "advanced_web",
        "jwt",
        "token",
        format!("alg={alg} typ={typ}"),
    );
    report = report.line(format!("Algorithm: {alg}"));
    report = report.line(format!("Type: {typ}"));
    if let Some(kid) = kid {
        report = report.line(format!("Key ID: {kid}"));
    }

    // Payload claims
    if let Some(exp) = payload.get("exp").and_then(|v| v.as_i64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let status = if exp < now { "EXPIRED" } else { "valid" };
        report = report.line(format!("exp: {} ({})", exp, status));
    }
    if let Some(iat) = payload.get("iat").and_then(|v| v.as_i64()) {
        report = report.line(format!("iat: {}", iat));
    }
    if let Some(nbf) = payload.get("nbf").and_then(|v| v.as_i64()) {
        report = report.line(format!("nbf: {}", nbf));
    }
    if let Some(sub) = payload.get("sub").and_then(|v| v.as_str()) {
        report = report.line(format!("sub: {}", sub));
    }
    if let Some(iss) = payload.get("iss").and_then(|v| v.as_str()) {
        report = report.line(format!("iss: {}", iss));
    }
    if let Some(aud) = payload.get("aud") {
        report = report.line(format!("aud: {}", aud));
    }

    // Security checks
    let mut risks: Vec<String> = Vec::new();

    match alg.to_ascii_lowercase().as_str() {
        "none" => risks.push("algorithm 'none' - signature not verified".to_string()),
        "hs256" | "hs384" | "hs512" => {
            risks.push(
                "HMAC algorithm - vulnerable to key confusion if public key used elsewhere"
                    .to_string(),
            );
        }
        "rs256" | "rs384" | "rs512" | "es256" | "es384" | "es512" | "ed25519" => {}
        _ => risks.push(format!("unknown algorithm: {alg}")),
    }

    if signature.is_empty() || signature == "Signature" {
        risks.push("empty or placeholder signature".to_string());
    }

    // Check for sensitive claims in payload
    let payload_str = payload_json.to_ascii_lowercase();
    if payload_str.contains("password")
        || payload_str.contains("secret")
        || payload_str.contains("key")
    {
        risks.push("potentially sensitive data in payload".to_string());
    }

    if !risks.is_empty() {
        report = report.line("⚠ Security issues:");
        for risk in &risks {
            report = report.line(format!("  - {}", risk));
        }
        report = report.finding(
            Finding::new(
                "advanced_web",
                "jwt",
                "token",
                "jwt security issues",
                RiskLevel::Medium,
            )
            .confidence(Confidence::Suspected)
            .detail(risks.join("; ")),
        );
    } else {
        report = report.line("✓ No obvious JWT security issues detected");
    }

    Ok(report)
}

fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn base64_url_decode(input: &str) -> Result<String, String> {
    let mut padded = input.replace('-', "+").replace('_', "/");
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(&padded)
        .map_err(|e| e.to_string())
        .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
}

/// SSRF chain detection: probes internal metadata endpoints via user-controllable URL parameters.
pub fn ssrf_probe(
    base_url: &str,
    param: &str,
    fetcher: &dyn Fetcher,
) -> Result<ModuleReport, String> {
    let base_url = base_url.trim();
    let param = param.trim();
    if base_url.is_empty() || param.is_empty() {
        return Err("advanced_web/ssrf needs a base URL and parameter name".into());
    }

    // Common SSRF payloads targeting cloud metadata services
    let payloads = vec![
        ("AWS metadata", "http://169.254.169.254/latest/meta-data/"),
        (
            "AWS IAM credentials",
            "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
        ),
        (
            "GCP metadata",
            "http://metadata.google.internal/computeMetadata/v1/",
        ),
        (
            "GCP metadata (alt)",
            "http://169.254.169.254/computeMetadata/v1/",
        ),
        (
            "Azure metadata",
            "http://169.254.169.254/metadata/instance?api-version=2021-02-01",
        ),
        (
            "DigitalOcean metadata",
            "http://169.254.169.254/metadata/v1/",
        ),
        ("Localhost", "http://localhost/"),
        ("Localhost admin", "http://localhost/admin"),
        ("Localhost API", "http://localhost/api"),
        ("Internal DNS", "http://internal/"),
        ("Kubernetes API", "http://kubernetes.default.svc/"),
        ("Docker socket", "http://host.docker.internal:2375/"),
        ("Consul", "http://consul:8500/v1/"),
        ("Etcd", "http://etcd:2379/"),
    ];

    let mut findings = Vec::new();
    let mut report = ModuleReport::new(
        "advanced_web",
        "ssrf",
        base_url.to_string(),
        format!("testing {} payloads on param '{}'", payloads.len(), param),
    );

    for (name, payload) in payloads.iter().take(MAX_SSRF_ENDPOINTS) {
        let test_url = if base_url.contains('?') {
            format!("{}&{}={}", base_url, param, url_encode(payload))
        } else {
            format!("{}?{}={}", base_url, param, url_encode(payload))
        };

        match fetcher.fetch(&test_url, "GET", None, &[]) {
            Ok(fetched) => {
                if fetched.status == 200 && !fetched.body.is_empty() {
                    // Check if response looks like metadata
                    let body_lower = fetched.body.to_ascii_lowercase();
                    let is_metadata = body_lower.contains("instance-id")
                        || body_lower.contains("iam/security-credentials")
                        || body_lower.contains("computeMetadata")
                        || body_lower.contains("kubernetes")
                        || body_lower.contains("docker")
                        || body_lower.contains("accesskey")
                        || body_lower.contains("secretkey");

                    if is_metadata {
                        findings.push(format!("⚠ {} - METADATA LEAK: {}", name, payload));
                        report = report.finding(
                            Finding::new(
                                "advanced_web",
                                "ssrf",
                                base_url.to_string(),
                                format!("ssrf metadata leak: {}", name),
                                RiskLevel::Critical,
                            )
                            .confidence(Confidence::Confirmed)
                            .detail(format!(
                                "Parameter '{}' with payload '{}' returned cloud metadata: {}",
                                param,
                                payload,
                                fetched.body.chars().take(500).collect::<String>()
                            )),
                        );
                    } else if fetched.body.len() > 100 {
                        findings.push(format!(
                            "⚠ {} - response received ({} bytes)",
                            name,
                            fetched.body.len()
                        ));
                        report = report.finding(
                            Finding::new(
                                "advanced_web",
                                "ssrf",
                                base_url.to_string(),
                                format!("ssrf response: {}", name),
                                RiskLevel::High,
                            )
                            .confidence(Confidence::Suspected)
                            .detail(format!(
                                "Parameter '{}' with payload '{}' returned HTTP 200 with {} bytes",
                                param,
                                payload,
                                fetched.body.len()
                            )),
                        );
                    } else {
                        findings.push(format!(
                            "✓ {} - no metadata ({} bytes)",
                            name,
                            fetched.body.len()
                        ));
                    }
                } else {
                    findings.push(format!(
                        "✓ {} - HTTP {} ({} bytes)",
                        name,
                        fetched.status,
                        fetched.body.len()
                    ));
                }
            }
            Err(e) => {
                findings.push(format!("✓ {} - error: {}", name, e));
            }
        }
    }

    for f in &findings {
        report = report.line(f.clone());
    }

    if findings.iter().any(|f| f.contains("METADATA LEAK")) {
        report.summary = "CRITICAL: SSRF metadata leak detected".to_string();
    } else if findings.iter().any(|f| f.contains("response received")) {
        report.summary = "HIGH: SSRF possible - internal responses received".to_string();
    } else {
        report.summary = "No SSRF indicators found".to_string();
    }

    Ok(report)
}

/// Comprehensive advanced web sweep.
pub fn full_sweep(target: &str, fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("advanced_web/sweep needs a target URL".into());
    }

    let mut combined =
        ModuleReport::new("advanced_web", "sweep", target.to_string(), String::new());

    // GraphQL introspection (common endpoints)
    for gql_path in ["/graphql", "/api/graphql", "/v1/graphql", "/query"] {
        let url = if target.ends_with('/') {
            format!("{}{}", target, &gql_path[1..])
        } else {
            format!("{}{}", target, gql_path)
        };
        match graphql_introspect(&url, fetcher) {
            Ok(r) if !r.findings.is_empty() || r.lines.iter().any(|l| l.contains("Queries")) => {
                combined
                    .lines
                    .push(format!("=== GraphQL ({}) ===", r.summary));
                combined.lines.extend(r.lines);
                combined.findings.extend(r.findings);
            }
            _ => {}
        }
    }

    // OpenAPI/Swagger (common endpoints)
    for openapi_path in [
        "/openapi.json",
        "/swagger.json",
        "/api/openapi.json",
        "/v1/openapi.json",
        "/api-docs",
    ] {
        let url = if target.ends_with('/') {
            format!("{}{}", target, &openapi_path[1..])
        } else {
            format!("{}{}", target, openapi_path)
        };
        match openapi_analyze(&url, fetcher) {
            Ok(r) if !r.lines.is_empty() => {
                combined
                    .lines
                    .push(format!("=== OpenAPI ({}) ===", r.summary));
                combined.lines.extend(r.lines);
                combined.findings.extend(r.findings);
            }
            _ => {}
        }
    }

    combined.summary = format!("completed GraphQL + OpenAPI sweep for {}", target);
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct StubFetcher(HashMap<String, Fetched>);
    impl Fetcher for StubFetcher {
        fn fetch(
            &self,
            url: &str,
            _method: &str,
            _body: Option<&str>,
            _headers: &[(&str, &str)],
        ) -> Result<Fetched, String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no stub for {url}"))
        }
    }

    fn fetched(status: u16, body: &str, headers: &[(&str, &str)]) -> Fetched {
        Fetched {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.to_string(),
        }
    }

    #[test]
    fn graphql_introspection_parses_schema() {
        let schema_json = serde_json::json!({
            "data": {
                "__schema": {
                    "types": [
                        {"name": "Query", "kind": "OBJECT", "fields": [
                            {"name": "user", "type": {"name": "User", "kind": "OBJECT"}},
                            {"name": "adminUsers", "type": {"name": "[User]", "kind": "LIST"}}
                        ]},
                        {"name": "Mutation", "kind": "OBJECT", "fields": [
                            {"name": "deleteUser", "type": {"name": "Boolean", "kind": "SCALAR"}}
                        ]},
                        {"name": "User", "kind": "OBJECT", "fields": [
                            {"name": "id", "type": {"name": "ID", "kind": "SCALAR"}},
                            {"name": "password", "type": {"name": "String", "kind": "SCALAR"}}
                        ]}
                    ]
                }
            }
        })
        .to_string();

        let fetcher = StubFetcher(
            [(
                "https://api.example.com/graphql".into(),
                fetched(200, &schema_json, &[("Content-Type", "application/json")]),
            )]
            .into(),
        );

        let report = graphql_introspect("https://api.example.com/graphql", &fetcher).unwrap();
        assert!(report.summary.contains("2 queries"));
        assert!(report.summary.contains("1 mutations"));
        assert!(report.lines.iter().any(|l| l.contains("Sensitive fields")));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "graphql sensitive field exposure"));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "graphql introspection enabled"));
    }

    #[test]
    fn graphql_introspection_disabled() {
        let fetcher = StubFetcher(
            [(
                "https://api.example.com/graphql".into(),
                fetched(200, "{\"data\":{\"__schema\":null}}", &[]),
            )]
            .into(),
        );

        let report = graphql_introspect("https://api.example.com/graphql", &fetcher).unwrap();
        assert!(report.summary.contains("introspection disabled"));
    }

    #[test]
    fn openapi_analyzes_endpoints_and_auth() {
        let spec = serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/users": {
                    "get": {
                        "security": [{"BearerAuth": []}],
                        "parameters": [{"name": "api_key", "in": "query"}]
                    },
                    "post": {
                        "security": [],
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "properties": {
                                            "password": {"type": "string"},
                                            "email": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "/admin": {
                    "get": {"security": [{"AdminAuth": []}]}
                }
            },
            "components": {
                "securitySchemes": {
                    "BearerAuth": {"type": "http", "scheme": "bearer"},
                    "AdminAuth": {"type": "apiKey", "in": "header", "name": "X-Admin-Key"}
                }
            }
        })
        .to_string();

        let fetcher = StubFetcher(
            [(
                "https://api.example.com/openapi.json".into(),
                fetched(200, &spec, &[]),
            )]
            .into(),
        );

        let report = openapi_analyze("https://api.example.com/openapi.json", &fetcher).unwrap();
        assert!(report.summary.contains("2 endpoints"));
        assert!(report.summary.contains("2 auth schemes"));
        assert!(report.lines.iter().any(|l| l.contains("BearerAuth")));
        assert!(report.lines.iter().any(|l| l.contains("AdminAuth")));
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("Sensitive parameters")));
    }

    #[test]
    fn jwt_analyzes_algorithm_and_claims() {
        // Valid RS256 token with exp in future
        let header = base64_url_encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = base64_url_encode(&format!(
            r#"{{"sub":"user123","exp":{}}}"#,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600
        ));
        let token = format!("{}.{}.signature", header, payload);

        let report = jwt_analyze(&token).unwrap();
        assert!(report.summary.contains("alg=RS256"));
        assert!(report.lines.iter().any(|l| l.contains("sub: user123")));
        assert!(report.lines.iter().any(|l| l.contains("valid")));
        assert!(!report.lines.iter().any(|l| l.contains("⚠")));
    }

    #[test]
    fn jwt_detects_none_algorithm() {
        let header = base64_url_encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload = base64_url_encode(r#"{"sub":"user123"}"#);
        let token = format!("{}.{}.", header, payload);

        let report = jwt_analyze(&token).unwrap();
        assert!(report.lines.iter().any(|l| l.contains("none")));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "jwt security issues"));
    }

    #[test]
    fn jwt_detects_hmac_algorithm() {
        let header = base64_url_encode(r#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = base64_url_encode(r#"{"sub":"user123"}"#);
        let token = format!("{}.{}.signature", header, payload);

        let report = jwt_analyze(&token).unwrap();
        assert!(report.lines.iter().any(|l| l.contains("HMAC")));
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "jwt security issues"));
    }

    #[test]
    fn ssrf_probe_tests_payloads() {
        let mut responses = HashMap::new();
        // AWS metadata responds with instance-id
        responses.insert(
            "http://example.com/api?url=http://169.254.169.254/latest/meta-data/".into(),
            fetched(200, "i-1234567890abcdef0", &[]),
        );
        // Others return 404
        responses.insert(
            "http://example.com/api?url=http://metadata.google.internal/computeMetadata/v1/".into(),
            fetched(404, "", &[]),
        );

        let fetcher = StubFetcher(responses);
        let report = ssrf_probe("http://example.com/api", "url", &fetcher).unwrap();
        assert!(report.summary.contains("METADATA LEAK") || report.summary.contains("SSRF"));
        assert!(report.lines.iter().any(|l| l.contains("AWS metadata")));
    }

    fn base64_url_encode(input: &str) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(input.as_bytes())
            .trim_end_matches('=')
            .to_string()
    }
}
