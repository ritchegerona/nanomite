//! Web tools: `web_fetch` (GET + readability-ish extraction), `web_search`
//! (DuckDuckGo Lite) and `ip_lookup` (public-IP geolocation/ISP/ASN via
//! ipapi.co). Sync by design — the runtime dispatches tool calls through
//! `tokio::task::spawn_blocking`.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;

const MAX_BODY: usize = 512 * 1024;
const MAX_CONTENT: usize = 24 * 1024;
const MAX_SNIPPET: usize = 240;
const MAX_RESULTS: usize = 8;
const UA: &str = "nanomite/0.1 (terminal coding agent)";
const IP_LOOKUP_ENDPOINT: &str = "https://ipapi.co";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchArgs {
    /// Absolute http(s) URL.
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchResult {
    pub url: String,
    pub status: u16,
    pub title: Option<String>,
    /// Text content: HTML stripped, entities decoded, blank lines collapsed.
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub query: String,
    pub results: Vec<WebSearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpLookupArgs {
    /// Public IPv4/IPv6 address. Omit (or pass "me") to look up this machine's
    /// own public IP.
    #[serde(default)]
    pub ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpLookupResult {
    pub ip: String,
    pub version: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub postal: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: Option<String>,
    pub asn: Option<String>,
    pub org: Option<String>,
    pub source: String,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

/// Entity names are assembled from parts so nothing strips them in transit.
fn entity(parts: &[char]) -> String {
    parts.iter().collect()
}

fn decode_entities(s: &str) -> String {
    let amp = entity(&['&', 'a', 'm', 'p', ';']);
    let lt = entity(&['&', 'l', 't', ';']);
    let gt = entity(&['&', 'g', 't', ';']);
    let quot = entity(&['&', 'q', 'u', 'o', 't', ';']);
    let apos = entity(&['&', '#', '3', '9', ';']);
    let nbsp = entity(&['&', 'n', 'b', 's', 'p', ';']);
    s.replace(&amp, "\u{26}")
        .replace(&lt, "\u{3c}")
        .replace(&gt, "\u{3e}")
        .replace(&quot, "\u{22}")
        .replace(&apos, "\u{27}")
        .replace(&nbsp, " ")
}

/// Rough HTML → text: drop script/style/head, keep line breaks, strip tags,
/// decode entities, collapse blank runs.
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    // Re-scan after every removal: earlier cuts shift the byte offsets.
    for tag in ["script", "style", "noscript", "svg", "head"] {
        loop {
            let lower = s.to_lowercase();
            let Some(a) = lower.find(&format!("<{tag}")) else {
                break;
            };
            let Some(rel) = lower[a..].find(&format!("</{tag}>")) else {
                break;
            };
            let end = a + rel + tag.len() + 3;
            if end > s.len() {
                break;
            }
            s.replace_range(a..end, "");
        }
    }

    let block_tags = [
        "<br", "</p", "<p", "</div", "<li", "</h", "<h1", "<h2", "<h3", "<h4", "<tr", "</tr",
    ];
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            let rest: String = chars[i..(i + 12).min(chars.len())].iter().collect();
            let lower_rest = rest.to_lowercase();
            if block_tags.iter().any(|t| lower_rest.starts_with(t)) {
                out.push('\n');
            }
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
        i += 1;
    }

    let out = decode_entities(&out);
    let mut collapsed = String::with_capacity(out.len());
    let mut blanks = 0usize;
    for line in out.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        collapsed.push_str(line.trim_end());
        collapsed.push('\n');
    }
    collapsed.trim().to_string()
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let a = lower.find("<title")?;
    let start = html[a..].find('>')? + a + 1;
    let end = html[start..].to_lowercase().find("</title")? + start;
    Some(decode_entities(html[start..end].trim()))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[derive(Clone)]
pub struct WebTools {
    /// When true, fetch/search/browse/screenshot refuse instead of reaching
    /// the network.
    pub offline: bool,
}

impl WebTools {
    pub fn new(offline: bool) -> Self {
        Self { offline }
    }

    pub fn fetch(&self, args: WebFetchArgs) -> Result<WebFetchResult, String> {
        if self.offline {
            return Err("refused: web_fetch is disabled in offline mode".into());
        }
        let url = args.url.trim().to_string();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("refused: only http(s) URLs are allowed".into());
        }
        let resp = client()?
            .get(&url)
            .send()
            .map_err(|e| format!("fetch failed: {e}"))?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().map_err(|e| format!("read failed: {e}"))?;
        let truncated = bytes.len() > MAX_BODY;
        let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY)]).into_owned();
        let title = extract_title(&body);
        let mut content = if body.trim_start().starts_with('<') {
            html_to_text(&body)
        } else {
            body
        };
        let content_truncated = content.chars().count() > MAX_CONTENT;
        content = content.chars().take(MAX_CONTENT).collect();
        Ok(WebFetchResult {
            url,
            status,
            title,
            content,
            truncated: truncated || content_truncated,
        })
    }

    pub fn search(&self, args: WebSearchArgs) -> Result<WebSearchResult, String> {
        if self.offline {
            return Err("refused: web_search is disabled in offline mode".into());
        }
        let query = args.query.trim().to_string();
        if query.is_empty() {
            return Err("refused: empty query".into());
        }
        let resp = client()?
            .post("https://lite.duckduckgo.com/lite/")
            .form(&[("q", query.as_str())])
            .send()
            .map_err(|e| format!("search failed: {e}"))?;
        let html = resp.text().map_err(|e| format!("read failed: {e}"))?;
        let results = parse_ddg_lite(&html);
        if results.is_empty() {
            return Err("no results parsed - the engine may be rate-limiting".into());
        }
        Ok(WebSearchResult {
            query,
            results: results.into_iter().take(MAX_RESULTS).collect(),
        })
    }

    /// Look up geolocation/ISP/ASN for a public IP, or for this machine's own
    /// public IP when no address (or "me") is supplied.
    pub fn ip_lookup(&self, args: IpLookupArgs) -> Result<IpLookupResult, String> {
        if self.offline {
            return Err("refused: ip_lookup is disabled in offline mode".into());
        }
        let requested = args
            .ip
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && s != "me");
        let (url, queried) = match requested {
            Some(ip) => {
                let parsed = IpAddr::from_str(&ip)
                    .map_err(|_| format!("refused: invalid IP address `{ip}`"))?;
                if !is_public_ip(parsed) {
                    return Err(format!("refused: `{ip}` is not a public IP address"));
                }
                (format!("{IP_LOOKUP_ENDPOINT}/{ip}/json/"), ip)
            }
            None => (format!("{IP_LOOKUP_ENDPOINT}/json/"), "me".to_string()),
        };
        let resp = client()?
            .get(&url)
            .send()
            .map_err(|e| format!("ip lookup failed: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ip lookup failed: HTTP {status} {}",
                crate::agent::runtime::shorten(&body, 120)
            ));
        }
        parse_ip_lookup(&body, &queried)
    }
}

/// Only public unicast addresses have meaningful geolocation data; rejecting
/// private/loopback/link-local/multicast input keeps the request well-formed
/// and avoids sending junk to the lookup service.
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(o[0] == 0
                || o[0] == 10
                || o[0] == 127
                || o[0] >= 224
                || (o[0] == 169 && o[1] == 254)
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 100 && (64..=127).contains(&o[1])))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.to_ipv4().is_some()
                || (first & 0xffc0) == 0xfe80
                || (first & 0xfe00) == 0xfc00
                || (first & 0xff00) == 0xff00)
        }
    }
}

/// Parse an ipapi.co JSON response into the bounded result the model sees.
fn parse_ip_lookup(body: &str, queried: &str) -> Result<IpLookupResult, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        format!(
            "ip lookup failed: unexpected response {}",
            crate::agent::runtime::shorten(body, 120)
        )
    })?;
    if value.get("error").and_then(|v| v.as_bool()) == Some(true) {
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ip lookup failed: {reason}"));
    }
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let number = |key: &str| value.get(key).and_then(|v| v.as_f64());
    Ok(IpLookupResult {
        ip: text("ip").unwrap_or_else(|| queried.to_string()),
        version: text("version"),
        city: text("city"),
        region: text("region"),
        country: text("country_name"),
        country_code: text("country"),
        postal: text("postal"),
        latitude: number("latitude"),
        longitude: number("longitude"),
        timezone: text("timezone"),
        asn: text("asn"),
        org: text("org"),
        source: "ipapi.co".to_string(),
    })
}

/// Parses DuckDuckGo Lite result rows without a regex dependency.
fn parse_ddg_lite(html: &str) -> Vec<WebSearchHit> {
    let anchor_marker = "class='result-link'";
    let snippet_marker = "class='result-snippet'";
    let mut hits = Vec::new();
    let mut cursor = 0usize;

    while hits.len() < MAX_RESULTS {
        let Some(rel) = html[cursor..].find(anchor_marker) else {
            break;
        };
        let seg_start = cursor + rel;

        // The <a ...> tag opening precedes the marker.
        let Some(tag_rel) = html[..seg_start].rfind("<a ") else {
            break;
        };
        let Some(href_rel) = html[tag_start(tag_rel)..seg_start].find("href=\"") else {
            cursor = seg_start + anchor_marker.len();
            continue;
        };
        let href_start = tag_start(tag_rel) + href_rel + 6;
        let Some(href_end_rel) = html[href_start..].find('"') else {
            break;
        };
        let url = decode_entities(&html[href_start..href_start + href_end_rel]);

        // Anchor text runs from the marker to </a>.
        let text_start = seg_start + anchor_marker.len();
        let Some(text_end_rel) = html[text_start..].find("</a>") else {
            break;
        };
        let text_end = text_start + text_end_rel;
        let mut title = strip_tags(&decode_entities(html[text_start..text_end].trim()));

        // Snippet: the next result-snippet cell after this anchor.
        let snippet = match html[text_end..].find(snippet_marker) {
            Some(s_rel) => {
                let open = text_end + s_rel;
                let Some(gt_rel) = html[open..].find('>') else {
                    break;
                };
                let start = open + gt_rel + 1;
                let Some(end_rel) = html[start..].find("</td>") else {
                    break;
                };
                let end = start + end_rel;
                strip_tags(&decode_entities(html[start..end].trim()))
            }
            None => String::new(),
        };
        let snippet: String = snippet.chars().take(MAX_SNIPPET).collect();
        if title.is_empty() {
            title = url.clone();
        }
        if url.starts_with("http") {
            hits.push(WebSearchHit {
                title,
                url,
                snippet,
            });
        }
        cursor = text_end;
    }
    hits
}

#[inline]
fn tag_start(tag_rel: usize) -> usize {
    tag_rel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_non_http_schemes_and_empty_queries() {
        let web = WebTools::new(false);
        assert!(web
            .fetch(WebFetchArgs {
                url: "ftp://example.com".into(),
            })
            .is_err());
        assert!(web
            .fetch(WebFetchArgs {
                url: "file:///etc/passwd".into(),
            })
            .is_err());
        assert!(web
            .search(WebSearchArgs {
                query: "   ".into(),
            })
            .is_err());
    }

    #[test]
    fn offline_refuses_fetch_and_search() {
        let web = WebTools::new(true);
        assert!(web
            .fetch(WebFetchArgs {
                url: "https://example.com".into(),
            })
            .is_err());
        assert!(web
            .search(WebSearchArgs {
                query: "test".into(),
            })
            .is_err());
        let err = web
            .fetch(WebFetchArgs {
                url: "https://example.com".into(),
            })
            .unwrap_err();
        assert!(err.contains("offline"), "{err}");
    }

    #[test]
    fn html_to_text_strips_and_decodes() {
        let amp = entity(&['&', 'a', 'm', 'p', ';']);
        let lt = entity(&['&', 'l', 't', ';']);
        let html = format!(
            "<html><head><script>evil()</script><title>T{amp}T</title></head>\
             <body><h1>Hi</h1><p>a <b> {lt}c</p><script>more()</script></body></html>"
        );
        let text = html_to_text(&html);
        assert!(text.contains("Hi"), "{text}");
        // `<` decodes to a literal '<', which survives as text (not a tag).
        assert!(text.contains("<c"), "decoded entity kept: {text}");
        assert!(!text.contains("evil"));
    }

    #[test]
    fn ddg_parser_extracts_links() {
        let raw_amp = entity(&['&', 'a', 'm', 'p', ';']);
        let html = format!(
            "<a rel=\"nofollow\" href=\"https://a.dev/x?a=1{raw_amp}b=2\" class='result-link'>A Site</a>\
             <td class='result-snippet'>first {raw_amp} result</td>\
             <a rel=\"nofollow\" href=\"https://b.dev\" class='result-link'>B Site</a>"
        );
        let hits = parse_ddg_lite(&html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, format!("https://a.dev/x?a=1{}b=2", "\u{26}"));
        assert_eq!(hits[0].title, "A Site");
        assert_eq!(hits[0].snippet, format!("first {} result", "\u{26}"));
        assert_eq!(hits[1].url, "https://b.dev");
    }

    #[test]
    fn ip_lookup_offline_refuses() {
        let err = WebTools::new(true)
            .ip_lookup(IpLookupArgs { ip: None })
            .unwrap_err();
        assert!(err.contains("offline"), "{err}");
    }

    #[test]
    fn ip_lookup_refuses_invalid_and_non_public_addresses() {
        let web = WebTools::new(false);
        for bad in [
            "not-an-ip",
            "192.168.1.1",
            "10.0.0.1",
            "172.16.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "::1",
            "fe80::1",
            "fc00::1",
        ] {
            let err = web
                .ip_lookup(IpLookupArgs {
                    ip: Some(bad.into()),
                })
                .unwrap_err();
            assert!(err.starts_with("refused:"), "{bad}: {err}");
        }
    }

    #[test]
    fn public_ip_classification() {
        for ok in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(IpAddr::from_str(ok).unwrap()), "{ok}");
        }
        for no in [
            "0.0.0.0",
            "10.1.2.3",
            "172.20.0.1",
            "192.168.0.1",
            "100.64.0.1",
            "224.0.0.1",
            "::",
            "fd00::1",
            "ff02::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_public_ip(IpAddr::from_str(no).unwrap()), "{no}");
        }
    }

    #[test]
    fn parses_ip_lookup_response() {
        let body = r#"{"ip":"8.8.8.8","version":"IPv4","city":"Mountain View","region":"California","country":"US","country_name":"United States","postal":"94043","latitude":37.38605,"longitude":-122.08385,"timezone":"America/Los_Angeles","asn":"AS15169","org":"Google LLC"}"#;
        let result = parse_ip_lookup(body, "8.8.8.8").unwrap();
        assert_eq!(result.ip, "8.8.8.8");
        assert_eq!(result.version.as_deref(), Some("IPv4"));
        assert_eq!(result.city.as_deref(), Some("Mountain View"));
        assert_eq!(result.country.as_deref(), Some("United States"));
        assert_eq!(result.country_code.as_deref(), Some("US"));
        assert_eq!(result.asn.as_deref(), Some("AS15169"));
        assert_eq!(result.org.as_deref(), Some("Google LLC"));
        assert_eq!(result.source, "ipapi.co");
        assert!((result.latitude.unwrap() - 37.38605).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_ip_lookup_error() {
        let err = parse_ip_lookup(r#"{"error":true,"reason":"RateLimited"}"#, "me").unwrap_err();
        assert!(err.contains("RateLimited"), "{err}");
    }
}
