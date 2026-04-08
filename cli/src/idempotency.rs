//! Idempotency key generation for safe request retries.

use sha2::{Digest, Sha256};

/// Generate a random idempotency key (16 random bytes, hex-encoded).
pub fn random_key() -> String {
    let bytes: [u8; 16] = rand::random();
    hex::encode(bytes)
}

/// Generate a deterministic idempotency key from a namespace and parts.
/// Used for operations where retries should be deduplicated (e.g., commit, reveal).
pub fn deterministic_key(namespace: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    for part in parts {
        hasher.update(b":");
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}
