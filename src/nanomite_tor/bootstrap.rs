//! Bootstrap progress parsing.
//!
//! Tor reports progress in two places: the log line
//! `Bootstrapped 80% (ap_conn_done): Connected to a relay` and the control-port
//! reply `GETINFO status/bootstrap-phase` →
//! `NOTICE BOOTSTRAP PROGRESS=80 TAG=ap_conn_done SUMMARY="Connected to a relay"`.
//! Both are parsed here without any I/O.

/// A snapshot of Tor's bootstrap progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapStatus {
    /// 0–100.
    pub percent: u8,
    /// Machine tag, e.g. `conn_done`, `done`.
    pub tag: String,
    /// Human summary, e.g. `Connected to a relay`.
    pub summary: String,
}

impl BootstrapStatus {
    pub fn new(percent: u8, tag: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            percent,
            tag: tag.into(),
            summary: summary.into(),
        }
    }

    /// True once the daemon is fully usable.
    pub fn done(&self) -> bool {
        self.percent >= 100
    }
}

impl Default for BootstrapStatus {
    fn default() -> Self {
        Self::new(0, "starting", "Starting")
    }
}

/// Parse a tor log line. Returns `None` for lines that are not bootstrap
/// progress (the vast majority of a tor log).
pub fn parse_log_line(line: &str) -> Option<BootstrapStatus> {
    let start = line.find("Bootstrapped ")? + "Bootstrapped ".len();
    let rest = &line[start..];
    let pct_end = rest.find('%')?;
    let percent: u8 = rest[..pct_end].trim().parse().ok()?;
    if percent > 100 {
        return None;
    }
    let after = &rest[pct_end + 1..];
    // Optional `(tag)` before the colon.
    let after = after.trim_start();
    let (tag, after) = if let Some(tag_body) = after.strip_prefix('(') {
        let close = tag_body.find(')')?;
        (tag_body[..close].to_string(), &tag_body[close + 1..])
    } else {
        (String::new(), after)
    };
    let summary = after
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    Some(BootstrapStatus::new(percent, tag, summary))
}

/// Parse the value of `GETINFO status/bootstrap-phase` (the part after the
/// first `=`), or a bare phase string.
pub fn parse_phase(value: &str) -> Option<BootstrapStatus> {
    // Strip a `status/bootstrap-phase=` prefix, but never split a bare phase
    // string at the first `PROGRESS=`.
    let value = match value.split_once('=') {
        Some((prefix, rest)) if !prefix.contains(char::is_whitespace) => rest,
        _ => value,
    }
    .trim();
    // `NOTICE BOOTSTRAP PROGRESS=80 TAG=ap_conn_done SUMMARY="Connected"`
    if !value.contains("BOOTSTRAP") {
        return None;
    }
    let mut percent = None;
    let mut tag = String::new();
    let mut summary = String::new();
    let tokens = tokenize(value);
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if let Some(v) = token.strip_prefix("PROGRESS=") {
            percent = v.parse::<u8>().ok();
        } else if let Some(v) = token.strip_prefix("TAG=") {
            tag = v.to_string();
        } else if let Some(v) = token.strip_prefix("SUMMARY=") {
            summary = v.to_string();
        }
        index += 1;
    }
    let percent = percent?;
    (percent <= 100).then(|| BootstrapStatus::new(percent, tag, summary))
}

/// Splits a control reply into whitespace-separated tokens, keeping quoted
/// strings (which may contain spaces) intact and unquoting them.
pub(crate) fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_line_with_tag() {
        let line =
            "Nov 08 12:00:00.000 [notice] Bootstrapped 80% (ap_conn_done): Connected to a relay";
        let status = parse_log_line(line).expect("parsed");
        assert_eq!(status.percent, 80);
        assert_eq!(status.tag, "ap_conn_done");
        assert_eq!(status.summary, "Connected to a relay");
        assert!(!status.done());
    }

    #[test]
    fn log_line_done_without_tag() {
        let line = "[notice] Bootstrapped 100%: Done";
        let status = parse_log_line(line).expect("parsed");
        assert_eq!(status.percent, 100);
        assert!(status.tag.is_empty());
        assert_eq!(status.summary, "Done");
        assert!(status.done());
    }

    #[test]
    fn non_bootstrap_lines_are_ignored() {
        assert!(parse_log_line("Nov 08 [notice] Opening Socks listener").is_none());
        assert!(parse_log_line("Bootstrapped nope%").is_none());
        assert!(parse_log_line("Bootstrapped 250% (x): too far").is_none());
    }

    #[test]
    fn phase_from_control_reply() {
        let value =
            "status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\"";
        let status = parse_phase(value).expect("parsed");
        assert_eq!(status.percent, 100);
        assert_eq!(status.tag, "done");
        assert_eq!(status.summary, "Done");
        assert!(status.done());
    }

    #[test]
    fn phase_without_equals_prefix() {
        let value = "NOTICE BOOTSTRAP PROGRESS=45 TAG=conn SUMMARY=\"Connecting to a relay\"";
        let status = parse_phase(value).expect("parsed");
        assert_eq!(status.percent, 45);
        assert_eq!(status.summary, "Connecting to a relay");
    }

    #[test]
    fn malformed_phase_is_none() {
        assert!(parse_phase("NOTICE BOOTSTRAP TAG=done").is_none());
        assert!(parse_phase("VERSION Tor=0.4.7").is_none());
    }
}
