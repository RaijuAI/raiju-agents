//! Commitment hash computation for the sealed commit-reveal protocol (ADR-004).

use sha2::{Digest, Sha256};

/// Domain separator prepended to all commitment hashes.
pub const DOMAIN_SEPARATOR: &[u8] = b"raiju-v1:";

/// Compute the commitment hash: SHA-256(domain_separator || prediction_bps_i32_be || nonce).
pub fn compute_hash(prediction_bps: u16, nonce: &[u8; 32]) -> String {
    let pred_bytes = i32::from(prediction_bps).to_be_bytes();
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_SEPARATOR);
    hasher.update(pred_bytes);
    hasher.update(nonce);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// H-10 + H-14: Verify hash matches the shared test vectors from
    /// docs/test-vectors/commitment-hash.json. These vectors are also
    /// verified by raiju-types (server) and the Python SDK.
    #[test]
    fn test_audit_h10_commitment_hash_test_vectors() {
        // Vector 1: 72% forecast, 0xAA nonce
        let nonce: [u8; 32] = [0xAA; 32];
        assert_eq!(
            compute_hash(7200, &nonce),
            "e37bd54454dd1c34d39175ad705ba757dcf1ff7bb0d88ac44b2343d976b214e2"
        );

        // Vector 2: 0% forecast, zero nonce
        let nonce: [u8; 32] = [0x00; 32];
        assert_eq!(
            compute_hash(0, &nonce),
            "b947e135b598a4c88194bd667d7c7272c62d6dc0330bfcd539b260c5cffb6c49"
        );

        // Vector 3: 100% forecast, 0xFF nonce
        let nonce: [u8; 32] = [0xFF; 32];
        assert_eq!(
            compute_hash(10000, &nonce),
            "f481e72a1b60675d121415e88ab1fc410353f578bc705c991532fec685b470a8"
        );

        // Vector 4: 50% forecast, 0xDEADBEEF pattern nonce
        let nonce_vec =
            hex::decode("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
                .unwrap();
        let nonce: [u8; 32] = nonce_vec.try_into().unwrap();
        assert_eq!(
            compute_hash(5000, &nonce),
            "3a40b116b4687e514e05c98e6c139385934227728ca2dc1c26ad0fe4916e0a44"
        );
    }

    /// H-14: Domain separator is included in hash computation.
    #[test]
    fn test_audit_h14_domain_separator_included() {
        let nonce: [u8; 32] = [0xAA; 32];
        let with_separator = compute_hash(7200, &nonce);

        // Compute WITHOUT domain separator
        let pred_bytes = (7200i32).to_be_bytes();
        let mut hasher = Sha256::new();
        hasher.update(pred_bytes);
        hasher.update(nonce);
        let without_separator = hex::encode(hasher.finalize());

        assert_ne!(
            with_separator, without_separator,
            "hash must differ with/without domain separator"
        );
    }

    /// H-14: Prediction encoding is big-endian i32.
    #[test]
    fn test_audit_h14_prediction_encoding() {
        assert_eq!((7200i32).to_be_bytes(), [0x00, 0x00, 0x1C, 0x20]);
        assert_eq!((0i32).to_be_bytes(), [0x00, 0x00, 0x00, 0x00]);
        assert_eq!((10000i32).to_be_bytes(), [0x00, 0x00, 0x27, 0x10]);
    }
}
