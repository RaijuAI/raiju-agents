//! Nonce storage for commit-reveal protocol.
//!
//! Stores nonces in ~/.raiju/nonces/<agent_id>/<market_id>.json.
//! Agent-namespaced to prevent collisions when running multiple agents.
//! The nonce is generated during commit and retrieved during reveal.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredNonce {
    pub prediction_bps: u16,
    pub nonce: String,
}

/// Validate that an ID is a valid UUID to prevent path traversal.
/// Rejects anything containing path separators, dots, or non-hex characters.
fn validate_uuid(id: &str, name: &str) -> Result<()> {
    // UUID format: 8-4-4-4-12 hex characters with hyphens
    if id.len() != 36 {
        anyhow::bail!("invalid {name}: expected UUID format, got '{id}'");
    }
    for (i, c) in id.chars().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != '-' {
                    anyhow::bail!("invalid {name}: expected '-' at position {i}");
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    anyhow::bail!("invalid {name}: non-hex character '{c}' at position {i}");
                }
            }
        }
    }
    Ok(())
}

/// Returns the nonce file path: ~/.raiju/nonces/<agent_id>/<market_id>.json
fn nonce_path(agent_id: &str, market_id: &str) -> Result<PathBuf> {
    validate_uuid(agent_id, "agent_id")?;
    validate_uuid(market_id, "market_id")?;
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".raiju").join("nonces").join(agent_id);
    std::fs::create_dir_all(&dir).context("failed to create nonce directory")?;
    Ok(dir.join(format!("{market_id}.json")))
}

pub fn store(agent_id: &str, market_id: &str, prediction_bps: u16, nonce: &str) -> Result<()> {
    let path = nonce_path(agent_id, market_id)?;
    let data = StoredNonce { prediction_bps, nonce: nonce.to_string() };
    std::fs::write(&path, serde_json::to_string(&data)?).context("failed to store nonce")?;
    Ok(())
}

pub fn load(agent_id: &str, market_id: &str) -> Result<StoredNonce> {
    let path = nonce_path(agent_id, market_id)?;
    let content = std::fs::read_to_string(&path)
        .context("No stored nonce found. Did you call raiju_commit first?")?;
    let stored: StoredNonce = serde_json::from_str(&content)?;
    Ok(stored)
}

pub fn remove(agent_id: &str, market_id: &str) -> Result<()> {
    let path = nonce_path(agent_id, market_id)?;
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to serialize tests that modify the HOME env var.
    // Tests run in the same process and env vars are global state.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    /// Run a closure with HOME temporarily set to a custom path.
    /// The lock ensures no two tests modify HOME concurrently.
    ///
    /// SAFETY: `set_var` / `remove_var` are unsafe in Rust 2024 because
    /// they mutate process-global state. The mutex guarantees no concurrent
    /// access within this test suite, and these tests are the only code
    /// in this process that modifies HOME.
    fn with_temp_home<F: FnOnce()>(f: F) {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();
        // SAFETY: protected by HOME_LOCK mutex, no concurrent env access.
        unsafe { std::env::set_var("HOME", tmp.path()) };
        f();
        // Restore original HOME
        match original_home {
            // SAFETY: protected by HOME_LOCK mutex.
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    // Valid UUIDs for tests
    const TEST_AGENT: &str = "11111111-1111-1111-1111-111111111111";
    const TEST_AGENT2: &str = "22222222-2222-2222-2222-222222222222";
    const TEST_MARKET: &str = "550e8400-e29b-41d4-a716-446655440000";
    const TEST_MARKET2: &str = "550e8400-e29b-41d4-a716-446655440001";

    #[test]
    fn test_audit_f8_rejects_path_traversal() {
        assert!(validate_uuid("../../../etc/passwd", "id").is_err());
        assert!(validate_uuid("foo/bar", "id").is_err());
        assert!(validate_uuid("..", "id").is_err());
        assert!(validate_uuid("", "id").is_err());
        assert!(validate_uuid("not-a-uuid", "id").is_err());
        assert!(validate_uuid("/absolute/path", "id").is_err());
    }

    #[test]
    fn test_audit_f8_accepts_valid_uuid() {
        assert!(validate_uuid(TEST_MARKET, "market_id").is_ok());
        assert!(validate_uuid("00000000-0000-0000-0000-000000000000", "id").is_ok());
    }

    #[test]
    fn test_audit_f8_store_rejects_traversal() {
        with_temp_home(|| {
            assert!(store(TEST_AGENT, "../../../etc/passwd", 5000, "nonce").is_err());
            assert!(store("../../../etc/passwd", TEST_MARKET, 5000, "nonce").is_err());
        });
    }

    #[test]
    fn store_creates_file_with_correct_json() {
        with_temp_home(|| {
            store(TEST_AGENT, TEST_MARKET, 7500, "deadbeef").unwrap();

            let path = nonce_path(TEST_AGENT, TEST_MARKET).unwrap();
            assert!(path.exists(), "nonce file should be created");

            let content = std::fs::read_to_string(&path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(parsed["prediction_bps"], 7500);
            assert_eq!(parsed["nonce"], "deadbeef");
        });
    }

    #[test]
    fn store_overwrites_existing_nonce() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-446655440002";
            store(TEST_AGENT, market, 5000, "first-nonce").unwrap();
            store(TEST_AGENT, market, 8000, "second-nonce").unwrap();

            let stored = load(TEST_AGENT, market).unwrap();
            assert_eq!(stored.prediction_bps, 8000);
            assert_eq!(stored.nonce, "second-nonce");
        });
    }

    #[test]
    fn store_handles_zero_bps() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-446655440003";
            store(TEST_AGENT, market, 0, "some-nonce").unwrap();
            let stored = load(TEST_AGENT, market).unwrap();
            assert_eq!(stored.prediction_bps, 0);
        });
    }

    #[test]
    fn store_handles_max_u16_bps() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-446655440004";
            store(TEST_AGENT, market, u16::MAX, "some-nonce").unwrap();
            let stored = load(TEST_AGENT, market).unwrap();
            assert_eq!(stored.prediction_bps, u16::MAX);
        });
    }

    #[test]
    fn store_handles_empty_nonce_string() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-446655440005";
            store(TEST_AGENT, market, 5000, "").unwrap();
            let stored = load(TEST_AGENT, market).unwrap();
            assert_eq!(stored.nonce, "");
        });
    }

    #[test]
    fn load_round_trip_with_store() {
        with_temp_home(|| {
            let market_id = "550e8400-e29b-41d4-a716-446655440006";
            let prediction_bps = 7200u16;
            let nonce_hex = "aabbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344";

            store(TEST_AGENT, market_id, prediction_bps, nonce_hex).unwrap();
            let stored = load(TEST_AGENT, market_id).unwrap();

            assert_eq!(stored.prediction_bps, prediction_bps);
            assert_eq!(stored.nonce, nonce_hex);
        });
    }

    #[test]
    fn load_missing_file_returns_error() {
        with_temp_home(|| {
            let result = load(TEST_AGENT, "550e8400-e29b-41d4-a716-446655440007");
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("No stored nonce found"),
                "Expected helpful error message, got: {err_msg}"
            );
        });
    }

    #[test]
    fn remove_deletes_stored_nonce() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-44665544000c";
            store(TEST_AGENT, market, 5000, "some-nonce").unwrap();

            // Verify it exists first
            assert!(load(TEST_AGENT, market).is_ok());

            remove(TEST_AGENT, market).unwrap();

            // Should no longer be loadable
            assert!(load(TEST_AGENT, market).is_err());
        });
    }

    #[test]
    fn remove_nonexistent_file_is_idempotent() {
        with_temp_home(|| {
            // Removing a file that never existed should not error
            let result = remove(TEST_AGENT, "550e8400-e29b-41d4-a716-44665544000d");
            assert!(result.is_ok(), "removing nonexistent nonce should succeed silently");
        });
    }

    #[test]
    fn store_multiple_markets_independently() {
        with_temp_home(|| {
            store(TEST_AGENT, TEST_MARKET, 1000, "nonce-a").unwrap();
            store(TEST_AGENT, TEST_MARKET2, 9000, "nonce-b").unwrap();

            let a = load(TEST_AGENT, TEST_MARKET).unwrap();
            let b = load(TEST_AGENT, TEST_MARKET2).unwrap();

            assert_eq!(a.prediction_bps, 1000);
            assert_eq!(a.nonce, "nonce-a");
            assert_eq!(b.prediction_bps, 9000);
            assert_eq!(b.nonce, "nonce-b");
        });
    }

    #[test]
    fn different_agents_have_separate_nonces() {
        with_temp_home(|| {
            // Same market, different agents - should not collide
            store(TEST_AGENT, TEST_MARKET, 3000, "agent1-nonce").unwrap();
            store(TEST_AGENT2, TEST_MARKET, 7000, "agent2-nonce").unwrap();

            let a = load(TEST_AGENT, TEST_MARKET).unwrap();
            let b = load(TEST_AGENT2, TEST_MARKET).unwrap();

            assert_eq!(a.prediction_bps, 3000);
            assert_eq!(a.nonce, "agent1-nonce");
            assert_eq!(b.prediction_bps, 7000);
            assert_eq!(b.nonce, "agent2-nonce");
        });
    }

    #[test]
    fn load_corrupted_json_returns_error() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-446655440008";
            let path = nonce_path(TEST_AGENT, market).unwrap();
            std::fs::write(&path, "not valid json {{{").unwrap();

            let result = load(TEST_AGENT, market);
            assert!(result.is_err(), "corrupted JSON should produce an error");
        });
    }

    #[test]
    fn load_empty_file_returns_error() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-446655440009";
            let path = nonce_path(TEST_AGENT, market).unwrap();
            std::fs::write(&path, "").unwrap();

            let result = load(TEST_AGENT, market);
            assert!(result.is_err(), "empty file should produce an error");
        });
    }

    #[test]
    fn load_json_missing_fields_returns_error() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-44665544000a";
            let path = nonce_path(TEST_AGENT, market).unwrap();
            std::fs::write(&path, r#"{"prediction_bps": 5000}"#).unwrap();

            let result = load(TEST_AGENT, market);
            assert!(result.is_err(), "JSON missing 'nonce' field should produce an error");
        });
    }

    #[test]
    fn remove_double_delete_is_idempotent() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-44665544000e";
            store(TEST_AGENT, market, 5000, "nonce").unwrap();
            remove(TEST_AGENT, market).unwrap();

            // Second delete should also succeed
            let result = remove(TEST_AGENT, market);
            assert!(result.is_ok(), "double delete should succeed silently");
        });
    }

    #[test]
    fn stored_nonce_serialization_format() {
        with_temp_home(|| {
            let market = "550e8400-e29b-41d4-a716-44665544000f";
            store(TEST_AGENT, market, 7500, "hexnonce123").unwrap();
            let path = nonce_path(TEST_AGENT, market).unwrap();
            let raw = std::fs::read_to_string(&path).unwrap();
            let parsed: StoredNonce = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed.prediction_bps, 7500);
            assert_eq!(parsed.nonce, "hexnonce123");
        });
    }
}
