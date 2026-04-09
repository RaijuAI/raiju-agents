//! Nostr event building and secret key management (ADR-028).

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Load a Nostr secret key from a file path or the RAIJU_NOSTR_SECRET_KEY env var.
/// Returns None if neither is set and `required` is false.
pub fn load_secret_key(file_path: Option<&Path>, required: bool) -> Result<Option<String>> {
    if let Some(path) = file_path {
        let key = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read secret key file {}", path.display()))?
            .trim()
            .to_string();
        return Ok(Some(key));
    }

    match std::env::var("RAIJU_NOSTR_SECRET_KEY") {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_string())),
        _ if required => {
            anyhow::bail!(
                "Nostr signing requires RAIJU_NOSTR_SECRET_KEY or --nostr-secret-key-file"
            )
        }
        _ => Ok(None),
    }
}

/// Derive the x-only public key (hex) and keypair from a secret key hex string.
pub fn derive_pubkey(secret_key_hex: &str) -> Result<(String, secp256k1::Keypair)> {
    let sk_bytes = hex::decode(secret_key_hex).context("nostr secret key must be valid hex")?;
    let sk = secp256k1::SecretKey::from_slice(&sk_bytes)
        .context("nostr secret key must be a valid 32-byte secp256k1 key")?;
    let secp = secp256k1::Secp256k1::new();
    let keypair = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _) = keypair.x_only_public_key();
    Ok((hex::encode(xonly.serialize()), keypair))
}

/// Build and sign a Nostr event with the given kind and tags.
/// Works for kind 30150 (prediction proofs) and kind 27235 (NIP-98 auth).
pub fn build_event(
    secret_key_hex: &str,
    kind: u32,
    tags: serde_json::Value,
) -> Result<serde_json::Value> {
    let (pubkey_hex, keypair) = derive_pubkey(secret_key_hex)?;
    let created_at =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    // Event ID = SHA-256([0, pubkey, created_at, kind, tags, content])
    let serialized = serde_json::json!([0, pubkey_hex, created_at, kind, tags, ""]);
    let serialized_str = serde_json::to_string(&serialized)?;
    let event_id: [u8; 32] = Sha256::digest(serialized_str.as_bytes()).into();

    // BIP-340 Schnorr signature over the event ID
    let secp = secp256k1::Secp256k1::new();
    let msg = secp256k1::Message::from_digest(event_id);
    let sig = secp.sign_schnorr(&msg, &keypair);

    Ok(serde_json::json!({
        "id": hex::encode(event_id),
        "pubkey": pubkey_hex,
        "created_at": created_at,
        "kind": kind,
        "tags": tags,
        "content": "",
        "sig": hex::encode(sig.serialize()),
    }))
}
