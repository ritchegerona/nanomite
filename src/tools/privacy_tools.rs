//! Agent-facing tools for the VPN and Tor modules.
//!
//! [`dispatch`] is called by [`crate::agent::runtime::run_tool`] when the
//! model invokes `vpn_control` or `tor_control`. Both tools mutate the
//! machine's network state (local socket, optional TUN interface, a managed
//! `tor` daemon) and are classed [`crate::agent::policy::Risk::External`]:
//! they run in Build/Hacker mode and ask for confirmation in Plan mode.

#[cfg(any(feature = "vpn", feature = "tor"))]
use serde::Deserialize;
use serde_json::Value;
#[cfg(any(feature = "vpn", feature = "tor"))]
use std::path::Path;

pub fn privacy_tool_schemas() -> Vec<crate::agent::provider::Tool> {
    #[allow(unused_mut)]
    let mut schemas = Vec::new();
    #[cfg(feature = "vpn")]
    schemas.push(crate::agent::provider::Tool::new(
        "vpn_control",
        "Start, stop or inspect the built-in NanomiteVPN tunnel: binds a UDP socket, optionally creates a TUN interface and initiates peer handshakes. Changes the machine's network state.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "description": "start | stop | status" },
                "config_path": { "type": "string", "description": "Optional path to a vpn.toml config file" }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    ));
    #[cfg(feature = "tor")]
    schemas.push(crate::agent::provider::Tool::new(
        "tor_control",
        "Start, stop or inspect the built-in NanomiteTOR daemon: launches a local `tor` process, waits for bootstrap and reports the SOCKS/control ports. Changes the machine's network state.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "description": "start | stop | status" },
                "data_dir": { "type": "string", "description": "Optional Tor data directory" }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    ));
    schemas
}

/// Dispatch a privacy tool call. Returns `(payload_json, summary)`.
pub fn dispatch(call_name: &str, _args: Value) -> Result<(String, String), String> {
    match call_name {
        #[cfg(feature = "vpn")]
        "vpn_control" => {
            let parsed: VpnControlArgs = serde_json::from_value(_args)
                .map_err(|e| format!("bad vpn_control arguments: {e}"))?;
            let text = match parsed.action.trim().to_ascii_lowercase().as_str() {
                "start" => crate::privacy::vpn::start_blocking(
                    parsed.config_path.as_deref().map(Path::new),
                )?,
                "stop" => crate::privacy::vpn::stop_blocking()?,
                "status" => crate::privacy::vpn::status_blocking()?,
                other => {
                    return Err(format!(
                        "unknown vpn_control action `{other}` (use start, stop or status)"
                    ))
                }
            };
            Ok((serde_json::to_string(&text).unwrap(), text))
        }
        #[cfg(feature = "tor")]
        "tor_control" => {
            let parsed: TorControlArgs = serde_json::from_value(_args)
                .map_err(|e| format!("bad tor_control arguments: {e}"))?;
            let text = match parsed.action.trim().to_ascii_lowercase().as_str() {
                "start" => {
                    crate::privacy::tor::start_blocking(parsed.data_dir.as_deref().map(Path::new))?
                }
                "stop" => crate::privacy::tor::stop_blocking()?,
                "status" => crate::privacy::tor::status_blocking()?,
                other => {
                    return Err(format!(
                        "unknown tor_control action `{other}` (use start, stop or status)"
                    ))
                }
            };
            Ok((serde_json::to_string(&text).unwrap(), text))
        }
        _ => Err(format!("unknown privacy tool: {call_name}")),
    }
}

#[derive(Deserialize, Default)]
#[cfg(any(feature = "vpn", feature = "tor"))]
struct VpnControlArgs {
    action: String,
    #[serde(default)]
    config_path: Option<String>,
}

#[derive(Deserialize, Default)]
#[cfg(any(feature = "vpn", feature = "tor"))]
struct TorControlArgs {
    action: String,
    #[serde(default)]
    data_dir: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_unknown_tool_is_refused() {
        let err = dispatch("nope", serde_json::json!({})).unwrap_err();
        assert!(err.contains("unknown privacy tool"));
    }

    #[test]
    #[cfg(feature = "vpn")]
    fn dispatch_missing_action_is_refused() {
        let err = dispatch(
            "vpn_control",
            serde_json::json!({ "config_path": "/tmp/vpn.toml" }),
        )
        .unwrap_err();
        assert!(err.contains("bad vpn_control arguments") || err.contains("missing field"));
    }
}
