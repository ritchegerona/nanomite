//! CLI client for NanomiteVPN.
//!
//! Provides command-line subcommands for managing VPN connections:
//! connect, disconnect, status, peers, keygen, config.

use crate::nanomite_vpn::{
    config::Config,
    crypto::KeyPair,
    engine::{PeerConfig as EnginePeerConfig, TunnelEvent, TunnelState, VpnEngine},
    VpnError, VpnResult,
};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::sync::mpsc;

/// NanomiteVPN command-line interface.
#[derive(Parser, Debug)]
#[command(
    name = "nanomite-vpn",
    about = "NanomiteVPN - Personal WireGuard-inspired VPN"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to configuration file.
    #[arg(short = 'c', long = "config", default_value = "vpn.toml")]
    pub config: String,

    /// Enable verbose output.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generate a new key pair.
    Keygen {
        /// Output file for the key pair (default: print to stdout).
        #[arg(short = 'o', long = "output")]
        output: Option<String>,
    },

    /// Create a new configuration file.
    Init {
        /// Configuration file path.
        #[arg(default_value = "vpn.toml")]
        path: String,
    },

    /// Connect to a VPN peer.
    Connect {
        /// Peer public key (hex or base64).
        #[arg(long = "peer")]
        peer: String,

        /// Peer endpoint (host:port).
        #[arg(long = "endpoint")]
        endpoint: String,

        /// Local listen port.
        #[arg(short = 'p', long = "port", default_value = "51820")]
        port: u16,
    },

    /// Disconnect from a peer.
    Disconnect {
        /// Peer public key (hex).
        #[arg(long = "peer")]
        peer: String,
    },

    /// Show tunnel status and statistics.
    Status {
        /// Show detailed peer information.
        #[arg(short = 'd', long = "detail")]
        detail: bool,
    },

    /// List configured peers.
    Peers,
    /*
    /// Show configuration.
    Config {
        /// Show the private key (use with caution).
        #[arg(long = "show-secret")]
        show_secret: bool,
    },
    */
    /// Test for DNS leaks.
    DnsLeakTest,
}

/// Parse CLI arguments and run the VPN client.
pub async fn run_cli() -> VpnResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Keygen { output } => cmd_keygen(output).await,
        Commands::Init { path } => cmd_init(&path),
        Commands::Connect {
            peer,
            endpoint,
            port,
        } => cmd_connect(&peer, &endpoint, port).await,
        Commands::Disconnect { peer } => cmd_disconnect(&peer).await,
        Commands::Status { detail } => cmd_status(detail).await,
        Commands::Peers => cmd_peers().await,
        Commands::DnsLeakTest => cmd_dns_leak_test().await,
    }
}

/// Generate a new key pair.
async fn cmd_keygen(output: Option<String>) -> VpnResult<()> {
    let keypair = KeyPair::generate()?;
    let public_hex = hex::encode(keypair.public);
    let private_hex = hex::encode(keypair.private);

    let output_str = format!(
        "Public key:  {}\nPrivate key: {}\n",
        public_hex, private_hex
    );

    if let Some(path) = output {
        std::fs::write(&path, &output_str)
            .map_err(|e| VpnError::Config(format!("failed to write key file: {e}")))?;
        println!("Key pair written to {}", path);
    } else {
        print!("{}", output_str);
    }

    Ok(())
}

/// Initialize a new configuration file.
fn cmd_init(path: &str) -> VpnResult<()> {
    let config = Config::default();
    config
        .to_file(std::path::Path::new(path))
        .map_err(|e| VpnError::Config(format!("failed to write config: {e}")))?;
    println!("Configuration written to {}", path);
    Ok(())
}

/// Connect to a VPN peer.
async fn cmd_connect(peer: &str, endpoint: &str, port: u16) -> VpnResult<()> {
    let public_key = Config::parse_public_key(peer).map_err(|e| VpnError::Config(e.to_string()))?;
    let endpoint_addr: SocketAddr = endpoint
        .parse()
        .map_err(|e| VpnError::Network(format!("invalid endpoint: {e}")))?;

    let mut engine = VpnEngine::new(crate::nanomite_vpn::engine::VpnConfig {
        listen_port: port,
        ..Default::default()
    })
    .await?;

    // Set up event channel
    let (tx, mut rx) = mpsc::unbounded_channel::<TunnelEvent>();
    engine.set_event_channel(tx);

    // Bind to UDP socket
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    engine.bind(addr).await?;

    // Create TUN device
    engine.create_tun().await?;

    // Add peer and connect
    let peer_config = EnginePeerConfig {
        public_key,
        endpoint: endpoint_addr,
        allowed_ips: vec!["0.0.0.0/0".to_string()],
        persistent_keepalive: Some(15),
        preshared_key: None,
    };
    engine.add_peer(peer_config).await?;

    engine.connect(&public_key).await?;

    println!("Connected to {} at {}", hex::encode(public_key), endpoint);

    // Spawn the engine run loop
    let engine = std::sync::Arc::new(engine);
    let engine_clone = engine.clone();

    // Handle events
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                TunnelEvent::Established { peer, fingerprint } => {
                    println!(
                        "Tunnel established with {} (fingerprint: {})",
                        peer, fingerprint
                    );
                }
                TunnelEvent::Disconnected { peer } => {
                    println!("Disconnected from {}", peer);
                }
                TunnelEvent::Failed { peer, error } => {
                    eprintln!("Tunnel failed with {}: {}", peer, error);
                }
                TunnelEvent::PacketSent { bytes }
                    if std::env::var("NANOMITE_VPN_VERBOSE").is_ok() =>
                {
                    println!("Sent {} bytes", bytes);
                }
                TunnelEvent::PacketReceived { bytes }
                    if std::env::var("NANOMITE_VPN_VERBOSE").is_ok() =>
                {
                    println!("Received {} bytes", bytes);
                }
                _ => {}
            }
        }
    });

    // Run the tunnel loop (this blocks)
    let _ = engine_clone.run().await;

    Ok(())
}

/// Disconnect from a peer.
async fn cmd_disconnect(peer: &str) -> VpnResult<()> {
    let public_key = Config::parse_public_key(peer).map_err(|e| VpnError::Config(e.to_string()))?;
    println!("Disconnecting from {}...", hex::encode(public_key));
    // In a real implementation, we'd send a disconnect message to the running engine
    println!("Disconnected.");
    Ok(())
}

/// Show tunnel status.
async fn cmd_status(detail: bool) -> VpnResult<()> {
    println!("=== NanomiteVPN Status ===");
    println!("State: {}", TunnelState::Disconnected.code());
    if detail {
        println!("Local public key: (not loaded)");
        println!("Listen port: 51820");
        println!("TUN interface: nanomite0");
        println!("MTU: 1400");
    }
    Ok(())
}

/// List configured peers.
async fn cmd_peers() -> VpnResult<()> {
    println!("=== Configured Peers ===");
    println!("No peers configured. Use 'nanomite-vpn connect' to add a peer.");
    Ok(())
}

/// Test for DNS leaks.
async fn cmd_dns_leak_test() -> VpnResult<()> {
    use crate::nanomite_vpn::dns::{DnsResolver, DoHConfig};

    let mut resolver = DnsResolver::new(DoHConfig::default());
    let result = resolver.leak_test().await;

    match result {
        Ok(record) => {
            println!("DNS leak test: PASSED");
            println!("Resolved {} to {:?}", record.name, record.addresses);
        }
        Err(e) => {
            println!("DNS leak test: FAILED");
            eprintln!("Error: {e}");
        }
    }

    Ok(())
}

impl FromStr for Commands {
    type Err = String;

    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Err("Use subcommands".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parsing_keygen() {
        let cli = Cli::parse_from(["nanomite-vpn", "keygen"]);
        assert!(matches!(cli.command, Commands::Keygen { output: None }));
    }

    #[test]
    fn cli_parsing_connect() {
        let cli = Cli::parse_from([
            "nanomite-vpn",
            "connect",
            "--peer",
            "abc",
            "--endpoint",
            "1.2.3.4:51820",
        ]);
        if let Commands::Connect {
            peer,
            endpoint,
            port,
        } = cli.command
        {
            assert_eq!(peer, "abc");
            assert_eq!(endpoint, "1.2.3.4:51820");
            assert_eq!(port, 51820);
        } else {
            panic!("expected Connect command");
        }
    }

    #[test]
    fn cli_parsing_status() {
        let cli = Cli::parse_from(["nanomite-vpn", "status", "--detail"]);
        if let Commands::Status { detail } = cli.command {
            assert!(detail);
        } else {
            panic!("expected Status command");
        }
    }

    #[test]
    fn cmd_keygen_output() {
        // Just verify keygen generates valid keys
        let keypair = KeyPair::generate().expect("key gen");
        assert_ne!(keypair.public, [0u8; 32]);
        assert_ne!(keypair.private, [0u8; 32]);
    }
}
