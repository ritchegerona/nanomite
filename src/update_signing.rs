//! Authenticated self-update: signature verification before the running
//! binary is replaced.
//!
//! Threat model: an attacker who can MITM or compromise the release channel
//! must not be able to make `nanomite update` install an attacker-controlled
//! binary. Every update is gated on a detached signature produced with a
//! pre-shared secret (HMAC-SHA256, keyed by `NANOMITE_UPDATE_KEY`). A release
//! is signed by the maintainer; the client verifies before swapping the
//! binary. No key configured means the update is refused unless
//! `NANOMITE_UPDATE_ALLOW_UNSIGNED=1` is set.
//!
//! Signature file: `<tag>.sig` next to the tag (base64-encoded HMAC-SHA256).
//! The signed payload is the commit SHA the tag points at — verifying the
//! signature binds the update to an exact commit, not just a mutable tag ref.

use base64::engine::{general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Verification outcome for an update candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// The signature checked out: proceed with the update.
    Verified,
    /// No signature file exists for this candidate.
    Unsigned,
    /// A signature exists but it does not verify.
    Tampered,
    /// The signature file is malformed.
    Malformed,
}

/// The update-signing configuration resolved from environment.
#[derive(Debug, Clone, Default)]
pub struct UpdateSigning {
    /// The shared secret (raw bytes). `None` means no key is configured.
    pub key: Option<Vec<u8>>,
    /// When true, unsigned releases are still applied (dangerous, opt-in).
    pub allow_unsigned: bool,
}

impl UpdateSigning {
    /// Resolve signing material from `NANOMITE_UPDATE_KEY` /
    /// `NANOMITE_UPDATE_ALLOW_UNSIGNED`.
    pub fn from_env() -> Self {
        let key = std::env::var("NANOMITE_UPDATE_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .map(|k| k.into_bytes());
        let allow_unsigned = std::env::var("NANOMITE_UPDATE_ALLOW_UNSIGNED")
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        Self {
            key,
            allow_unsigned,
        }
    }

    /// Whether this build refuses to install unsigned updates.
    pub fn requires_signature(&self) -> bool {
        !self.allow_unsigned
    }
}

/// Computes the detached signature for a payload: HMAC-SHA256(key, payload),
/// base64-encoded.
pub fn sign_payload(key: &[u8], payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Verifies a base64 signature against a payload.
pub fn verify_signature(key: &[u8], payload: &str, signature_b64: &str) -> bool {
    let Ok(sig_bytes) = general_purpose::STANDARD.decode(signature_b64.trim()) else {
        return false;
    };
    let mut mac = match HmacSha256::new_from_slice(key) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(payload.as_bytes());
    mac.verify_slice(&sig_bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_then_verify_roundtrips() {
        let key = b"test-update-secret";
        let payload = "a1b2c3d4e5f6";
        let sig = sign_payload(key, payload);
        assert!(!sig.is_empty());
        assert!(verify_signature(key, payload, &sig));
    }

    #[test]
    fn matches_reference_python_hmac() {
        // Pins the wire format against an independent implementation so the
        // maintainer signing script (scripts/sign-release.sh) and this client
        // can never drift: base64(HMAC-SHA256(key, payload)).
        let sig = sign_payload(
            b"cross-impl-vector-key",
            "0123456789abcdef0123456789abcdef01234567",
        );
        assert_eq!(sig, "IpYRYgRFExLL4ZRfxfqyeK6qAcOtQHxjXzL7Mt7Iqdg=");
        assert!(verify_signature(
            b"cross-impl-vector-key",
            "0123456789abcdef0123456789abcdef01234567",
            &sig
        ));
    }

    #[test]
    fn wrong_key_rejects() {
        let payload = "deadbeef";
        let good = sign_payload(b"key-a", payload);
        assert!(!verify_signature(b"key-b", payload, &good));
    }

    #[test]
    fn tampered_payload_rejected() {
        let key = b"k";
        let sig = sign_payload(key, "original");
        assert!(!verify_signature(key, "original-tampered", &sig));
    }

    #[test]
    fn malformed_signature_rejected() {
        let key = b"k";
        assert!(!verify_signature(key, "anything", "not-valid-base64!!"));
        assert!(!verify_signature(key, "anything", ""));
    }

    #[test]
    fn default_requires_signature() {
        assert!(UpdateSigning::default().requires_signature());
    }

    #[test]
    fn allow_unsigned_does_not_require() {
        let s = UpdateSigning {
            allow_unsigned: true,
            ..Default::default()
        };
        assert!(!s.requires_signature());
    }
}
