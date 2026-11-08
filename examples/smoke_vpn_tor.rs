//! Offline smoke test for the NanomiteVPN and NanomiteTOR libraries.
//!
//! Run with:
//! ```sh
//! cargo run --example smoke_vpn_tor --features "vpn,tor"
//! ```
//!
//! It performs a full VPN handshake + encrypted round-trip between two peers
//! and exercises the Tor control/SOCKS/onion parsers without a daemon.

use nanomite::nanomite_tor::bootstrap;
use nanomite::nanomite_tor::circuit;
use nanomite::nanomite_tor::config::TorConfig;
use nanomite::nanomite_tor::control::{
    parse_protocol_info, ControlReply, ReplyAccumulator, ReplyProgress,
};
use nanomite::nanomite_tor::onion::{OnionKey, OnionPort, OnionRequest};
use nanomite::nanomite_tor::socks;
use nanomite::nanomite_vpn::crypto::{wire_nonce, KeyPair};
use nanomite::nanomite_vpn::protocol::HandshakeProtocol;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    vpn_smoke()?;
    tor_smoke()?;
    println!("SMOKE OK: vpn + tor");
    Ok(())
}

fn vpn_smoke() -> Result<(), Box<dyn Error>> {
    let initiator_id = KeyPair::generate()?;
    let responder_id = KeyPair::generate()?;
    let initiator_pub = initiator_id.public;
    let responder_pub = responder_id.public;

    let mut initiator = HandshakeProtocol::new(1, initiator_id, responder_pub, None)?;
    let mut responder = HandshakeProtocol::new(7, responder_id, initiator_pub, None)?;

    let init = initiator.create_initiation()?;
    let response = responder.handle_initiation(&init)?;
    initiator.handle_response(&response)?;

    let initiator_keys = initiator
        .session_keys()
        .ok_or("initiator has no session keys")?;
    let responder_keys = responder
        .session_keys()
        .ok_or("responder has no session keys")?;

    let message = b"nanomite vpn smoke payload";
    let ciphertext = initiator_keys.send_key.seal(
        &wire_nonce(initiator_keys.sending_index, 0),
        message,
        b"vpn-tunnel",
    )?;
    let plaintext = responder_keys.recv_key.open(
        &wire_nonce(responder_keys.receiving_index, 0),
        &ciphertext,
        b"vpn-tunnel",
    )?;
    if plaintext != message {
        return Err("vpn round-trip mismatch".into());
    }

    println!(
        "VPN OK: handshake + encrypted round-trip (ciphertext {} bytes, initiator fp {}, responder fp {})",
        ciphertext.len(),
        nanomite::nanomite_vpn::crypto::peer_fingerprint(&initiator_pub),
        nanomite::nanomite_vpn::crypto::peer_fingerprint(&responder_pub),
    );
    Ok(())
}

fn tor_smoke() -> Result<(), Box<dyn Error>> {
    // Config / torrc.
    let config = TorConfig {
        data_dir: std::env::temp_dir().join("nanomite-tor-smoke"),
        ..TorConfig::default()
    };
    config.validate()?;
    let torrc = config.to_torrc();
    if !torrc.contains("SocksPort 127.0.0.1:9050") {
        return Err("torrc missing SocksPort".into());
    }
    println!("TOR OK: torrc rendered ({} lines)", torrc.lines().count());

    // v3 onion key + address.
    let key = OnionKey::generate()?;
    let address = key.address();
    if !address.ends_with(".onion") || address.len() != 62 {
        return Err(format!("unexpected onion address: {address}").into());
    }
    let blob = key.to_key_blob();
    let reparsed = OnionKey::from_key_blob(&blob)?;
    if reparsed.public() != key.public() {
        return Err("onion key blob round-trip mismatch".into());
    }
    let request = OnionRequest::new(vec![OnionPort::new(80, "127.0.0.1:8080")]);
    println!(
        "TOR OK: onion {} (key round-trip, command: {})",
        address,
        request.to_command()
    );

    // Control reply framing: PROTOCOLINFO.
    let mut acc = ReplyAccumulator::new();
    for (index, line) in [
        "250-PROTOCOLINFO 1",
        "250-AUTH METHODS=COOKIE,SAFECOOKIE COOKIEFILE=\"/tmp/cookie\"",
        "250-VERSION Tor=\"0.4.8.10\"",
        "250 OK",
    ]
    .iter()
    .enumerate()
    {
        match acc.push(line)? {
            ReplyProgress::NeedMore => {
                if index == 3 {
                    return Err("reply did not complete".into());
                }
            }
            ReplyProgress::Complete(reply) => {
                let info = parse_protocol_info(&reply)?;
                if info.tor_version.as_deref() != Some("0.4.8.10") {
                    return Err("PROTOCOLINFO version mismatch".into());
                }
                println!(
                    "TOR OK: control PROTOCOLINFO (auth methods: {})",
                    info.methods.join(",")
                );
            }
        }
    }

    // Bootstrap phase parsing.
    let status = bootstrap::parse_phase(
        "status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\"",
    )
    .ok_or("bootstrap phase did not parse")?;
    if !status.done() {
        return Err("bootstrap not done".into());
    }
    println!("TOR OK: bootstrap {}% ({})", status.percent, status.tag);

    // Circuit-status data block.
    let reply = ControlReply {
        code: 250,
        lines: vec!["circuit-status=".into()],
        data: Some("1 BUILT $AAAA~guard,$BBBB~middle,$CCCC~exit PURPOSE=GENERAL\n".into()),
    };
    let circuits = circuit::parse_circuit_status(&reply.body());
    if circuits.len() != 1 || circuits[0].hops.len() != 3 {
        return Err("circuit-status parse mismatch".into());
    }
    println!(
        "TOR OK: circuits parsed ({} built, {} hops)",
        circuit::circuit_health(&circuits).built,
        circuits[0].hops.len()
    );

    // SOCKS5 wire encoding + reply parsing (no network).
    let request = socks::connect_request(&socks::SocksTarget::domain("example.com", 443))?;
    let bound = socks::parse_connect_reply(&[5, 0, 0, 1, 93, 184, 216, 34, 0x01, 0xbb])?;
    println!(
        "TOR OK: socks connect request {} bytes, mock bound {}",
        request.len(),
        bound
    );

    // Leak policy.
    let mut guard = nanomite::nanomite_tor::leak::TorLeakGuard::new("127.0.0.1:9050".parse()?);
    guard.set_active(true);
    if guard.route("93.184.216.34:443".parse()?) != nanomite::nanomite_tor::RouteDecision::ViaTor {
        return Err("leak guard did not route public traffic via Tor".into());
    }
    println!("TOR OK: leak guard routes public egress via Tor");

    Ok(())
}
