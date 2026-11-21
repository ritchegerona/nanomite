//! Deep Web / Dark Web access via the local Tor SOCKS5 proxy.
//!
//! Nanomite already ships a [`crate::privacy::tor`] supervisor that launches a
//! `tor` daemon and exposes a SOCKS5 proxy on `127.0.0.1:9050`. This module
//! adds three authorized operations on top of it:
//!
//! - `deepweb/search <query>` — passive deep-web search (DuckDuckGo HTML over
//!   Tor; no direct target, so no scope check is needed).
//! - `deepweb/browse <url>` — fetch a page through the Tor SOCKS5 proxy
//!   (supports `.onion` v2/v3 addresses and clearnet sites routed over Tor).
//! - `deepweb/validate-onion <address>` — pure, offline validation of a
//!   `.onion` v3 address shape (56-char base32 service id + `.onion`).
//!
//! All network traffic is routed through the Tor SOCKS5 proxy via reqwest's
//! `socks` feature. No traffic ever leaves the machine except through Tor, and
//! `.onion` addresses are only ever reachable through the proxy (they do not
//! resolve in DNS), so there is no DNS-rebinding or SSRF risk to internal
//! space.
//!
//! The `tor` daemon must be started separately (`/tor start`) before
//! `deepweb/browse` or `deepweb/search` can connect. This module never starts
//! Tor itself — it only consumes the SOCKS5 endpoint.

use std::time::Duration;

use super::ModuleReport;
use crate::hacker::findings::{Confidence, Finding};
use crate::hacker::risk::RiskLevel;

/// Default Tor SOCKS5 endpoint exposed by [`crate::privacy::tor`].
const DEFAULT_SOCKS_ADDR: &str = "127.0.0.1:9050";
/// Per-request timeout for deep-web fetches.
const FETCH_TIMEOUT_SECS: u64 = 30;
/// Max body bytes kept from a browse result.
const MAX_BODY_CHARS: usize = 64_000;

/// A fetched page: status, content type and bounded text body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedPage {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
}

/// Injectible fetch so tests never touch the network or Tor.
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<FetchedPage, String>;
}

/// Production fetcher: one GET through the Tor SOCKS5 proxy, redirects off,
/// body truncated.
pub struct TorFetcher;

impl Fetcher for TorFetcher {
    fn fetch(&self, url: &str) -> Result<FetchedPage, String> {
        let proxy_url = format!("socks5h://{DEFAULT_SOCKS_ADDR}");
        let client = reqwest::blocking::Client::builder()
            .proxy(reqwest::Proxy::all(&proxy_url).map_err(|e| format!("proxy config: {e}"))?)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .user_agent("nanomite-deepweb/1.0")
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let response = client
            .get(url)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = response
            .bytes()
            .map_err(|e| format!("read body failed: {e}"))?;
        let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY_CHARS)]).into_owned();
        Ok(FetchedPage {
            status,
            content_type,
            body,
        })
    }
}

/// Validate a `.onion` v3 address offline: shape check only, no network.
///
/// A v3 onion address is `base32(pubkey ‖ 2-byte checksum ‖ 0x03).onion`,
/// i.e. exactly 56 lowercase base32 characters before the `.onion` suffix.
/// Returns the service id (without `.onion`) when valid.
pub fn validate_onion(address: &str) -> Result<String, String> {
    let address = address.trim();
    let suffix = ".onion";
    let service_id = address
        .strip_suffix(suffix)
        .ok_or_else(|| format!("`{address}` does not end in `.onion`"))?;
    if service_id.len() != 56 {
        return Err(format!(
            "`{address}`: v3 onion service id must be 56 base32 chars, got {}",
            service_id.len()
        ));
    }
    if !service_id
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7'))
    {
        return Err(format!("`{address}`: service id is not valid base32"));
    }
    Ok(service_id.to_string())
}

/// Bounded text extraction: strip HTML tags crudely and collapse whitespace so
/// a browse result stays readable and small. Not a real HTML parser — good
/// enough for evidence capture.
fn extract_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Collapse runs of whitespace into single spaces.
    let collapsed: String = out.split_whitespace().collect::<Vec<&str>>().join(" ");
    collapsed.chars().take(MAX_BODY_CHARS).collect()
}

/// `deepweb/search <query>`: passive deep-web search over Tor (DuckDuckGo HTML).
pub fn search(query: &str, fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("deepweb/search needs a query".into());
    }
    let encoded = urlencoding::encode(query);
    let url = format!("https://duckduckgo.com/html/?q={encoded}");
    let page = fetcher.fetch(&url)?;
    let report = analyze_search_results(&page, query);
    Ok(report)
}

/// `deepweb/browse <url>`: fetch one page through Tor. `.onion` addresses are
/// validated first; clearnet URLs are passed straight to the proxy.
pub fn browse(url: &str, fetcher: &dyn Fetcher) -> Result<ModuleReport, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("deepweb/browse needs a url".into());
    }
    let host = extract_host(url);
    // `.onion` targets must be valid v3 addresses before we try to reach them.
    if host.ends_with(".onion") {
        validate_onion(&host)?;
    }
    let page = fetcher.fetch(url)?;
    let mut report = ModuleReport::new("deepweb", "browse", url, "fetched through tor");
    report = report.line(format!("status: {}", page.status));
    if let Some(ct) = &page.content_type {
        report = report.line(format!("content-type: {ct}"));
    }
    let text = extract_text(&page.body);
    if !text.is_empty() {
        let preview: String = text.chars().take(2000).collect();
        report = report.line(format!("body-preview: {preview}"));
    }
    report = report.finding(
        Finding::new(
            "deepweb",
            "browse",
            url,
            "page fetched via tor",
            RiskLevel::Informational,
        )
        .confidence(Confidence::Confirmed)
        .detail(format!(
            "fetched {url} through {DEFAULT_SOCKS_ADDR}; status {}; body {} chars",
            page.status,
            page.body.len()
        )),
    );
    Ok(report)
}

/// `deepweb/validate-onion <address>`: pure offline shape validation.
pub fn validate_onion_op(address: &str) -> ModuleReport {
    let address = address.trim();
    match validate_onion(address) {
        Ok(service_id) => {
            let mut report = ModuleReport::new(
                "deepweb",
                "validate-onion",
                address,
                "valid v3 onion address",
            );
            report = report.line(format!("service-id: {service_id}"));
            report = report.line(format!("full address: {address}"));
            report
        }
        Err(err) => {
            let mut report = ModuleReport::new(
                "deepweb",
                "validate-onion",
                address,
                "invalid v3 onion address",
            );
            report = report.line(format!("error: {err}"));
            report
        }
    }
}

fn analyze_search_results(page: &FetchedPage, query: &str) -> ModuleReport {
    let mut report = ModuleReport::new(
        "deepweb",
        "search",
        query,
        "deep web search results via tor",
    );
    report = report.line(format!("status: {}", page.status));
    let text = extract_text(&page.body);
    if text.is_empty() {
        report = report.line("(no results parsed)");
        return report;
    }
    // DuckDuckGo HTML result titles are wrapped in <a class="result__a">…
    // Crudely pull out candidate result lines by splitting on common separators.
    let results: Vec<&str> = text
        .split("  ")
        .map(|s| s.trim())
        .filter(|s| s.len() > 20)
        .take(10)
        .collect();
    for (i, result) in results.iter().enumerate() {
        report = report.line(format!(
            "result {}: {}",
            i + 1,
            result.chars().take(220).collect::<String>()
        ));
    }
    if results.is_empty() {
        report = report.line("(no distinct results parsed; raw body below)");
        let preview: String = text.chars().take(1500).collect();
        report = report.line(format!("raw-preview: {preview}"));
    }
    report = report.finding(
        Finding::new(
            "deepweb",
            "search",
            query,
            "deep web search executed",
            RiskLevel::Low,
        )
        .confidence(Confidence::Suspected)
        .detail(format!(
            "searched duckduckgo html over tor for: {query}; body {} chars",
            page.body.len()
        )),
    );
    report
}

/// Extracts a host from a URL or plain host string (mirrors the coordinator's
/// helper but kept local so this module stays self-contained).
fn extract_host(target: &str) -> String {
    let target = target.trim();
    let without_scheme = target.split_once("://").map(|(_, r)| r).unwrap_or(target);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_onion_accepts_valid_v3_address() {
        // 56 valid base32 chars.
        let id = "aaaqeayeaudaocajbifqydiob4ibceqtcqkrmfyydenbwha5dyp3kead";
        let addr = format!("{id}.onion");
        assert_eq!(validate_onion(&addr).unwrap(), id);
    }

    #[test]
    fn validate_onion_rejects_bad_addresses() {
        assert!(validate_onion("not-an-onion").is_err());
        assert!(validate_onion("example.com").is_err());
        assert!(validate_onion("short.onion").is_err());
        assert!(
            validate_onion("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ.onion").is_err()
        );
        assert!(validate_onion("").is_err());
    }

    #[test]
    fn extract_text_strips_tags_and_collapses_whitespace() {
        let html = "<html><body>  Hello   <b>world</b>! </body></html>";
        assert_eq!(extract_text(html), "Hello world!");
    }

    #[test]
    fn search_refuses_empty_query() {
        let stub = StubFetcher;
        assert!(search("", &stub).is_err());
    }

    #[test]
    fn browse_refuses_empty_url() {
        let stub = StubFetcher;
        assert!(browse("", &stub).is_err());
    }

    #[test]
    fn browse_rejects_invalid_onion_address() {
        let stub = StubFetcher;
        assert!(browse("http://short.onion/", &stub).is_err());
        // A clearnet (non-.onion) URL is not subject to onion validation and
        // is passed straight to the fetcher.
        let _ = browse("http://example.com/", &stub).unwrap();
    }

    #[test]
    fn browse_accepts_valid_onion_address() {
        let stub = StubFetcher;
        let id = "aaaqeayeaudaocajbifqydiob4ibceqtcqkrmfyydenbwha5dyp3kead";
        let report = browse(&format!("http://{id}.onion/"), &stub).unwrap();
        assert_eq!(report.summary, "fetched through tor");
        assert!(!report.findings.is_empty());
    }

    #[test]
    fn validate_onion_op_reports_valid_and_invalid() {
        let id = "aaaqeayeaudaocajbifqydiob4ibceqtcqkrmfyydenbwha5dyp3kead";
        let ok = validate_onion_op(&format!("{id}.onion"));
        assert!(ok.summary.contains("valid"));
        let bad = validate_onion_op("bad.onion");
        assert!(bad.summary.contains("invalid"));
    }

    struct StubFetcher;
    impl Fetcher for StubFetcher {
        fn fetch(&self, _url: &str) -> Result<FetchedPage, String> {
            Ok(FetchedPage {
                status: 200,
                content_type: Some("text/html".into()),
                body: "<html><body>Example result one here with enough text. Example result two here with more text content for testing.</body></html>".into(),
            })
        }
    }
}
