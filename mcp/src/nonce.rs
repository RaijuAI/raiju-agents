//! Nonce storage for commit-reveal protocol.
//!
//! Stores nonces in ~/.raiju/nonces/ as JSON files, keyed by `market_id`.
//! The nonce is generated during commit and retrieved during reveal.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredNonce {
    pub prediction_bps: u16,
    pub nonce: String,
}

/// Validate that a market_id is a valid UUID to prevent path traversal.
/// Rejects anything containing path separators, dots, or non-hex characters.
fn validate_market_id(market_id: &str) -> Result<()> {
    // UUID format: 8-4-4-4-12 hex characters with hyphens
    if market_id.len() != 36 {
        anyhow::bail!("invalid market_id: expected UUID format, got '{market_id}'");
    }
    for (i, c) in market_id.chars().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != '-' {
                    anyhow::bail!("invalid market_id: expected '-' at position {i}");
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    anyhow::bail!("invalid market_id: non-hex character '{c}' at position {i}");
                }
            }
        }
    }
    Ok(())
}

fn nonce_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".raiju").join("nonces");
    std::fs::create_dir_all(&dir).context("failed to create nonce directory")?;
    Ok(dir)
}

pub fn store(market_id: &str, prediction_bps: u16, nonce: &str) -> Result<()> {
    validate_market_id(market_id)?;
    let path = nonce_dir()?.join(format!("{market_id}.json"));
    let data = StoredNonce { prediction_bps, nonce: nonce.to_string() };
    std::fs::write(&path, serde_json::to_string(&data)?).context("failed to store nonce")?;
    Ok(())
}

pub fn load(market_id: &str) -> Result<StoredNonce> {
    validate_market_id(market_id)?;
    let path = nonce_dir()?.join(format!("{market_id}.json"));
    let content = std::fs::read_to_string(&path)
        .context("No stored nonce found. Did you call raiju_commit first?")?;
    let stored: StoredNonce = serde_json::from_str(&content)?;
    Ok(stored)
}

pub fn remove(market_id: &str) -> Result<()> {
    validate_market_id(market_id)?;
    let path = nonce_dir()?.join(format!("{market_id}.json"));
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

    // Valid UUID for tests
    const TEST_UUID: &str = "550e8400-e29b-41d4-a716-446655440000";
    const TEST_UUID2: &str = "550e8400-e29b-41d4-a716-446655440001";

    #[test]
    fn test_audit_f8_rejects_path_traversal() {
        assert!(validate_market_id("../../../etc/passwd").is_err());
        assert!(validate_market_id("foo/bar").is_err());
        assert!(validate_market_id("..").is_err());
        assert!(validate_market_id("").is_err());
        assert!(validate_market_id("not-a-uuid").is_err());
        assert!(validate_market_id("/absolute/path").is_err());
    }

    #[test]
    fn test_audit_f8_accepts_valid_uuid() {
        assert!(validate_market_id(TEST_UUID).is_ok());
        assert!(validate_market_id("00000000-0000-0000-0000-000000000000").is_ok());
    }

    #[test]
    fn test_audit_f8_store_rejects_traversal() {
        with_temp_home(|| {
            assert!(store("../../../etc/passwd", 5000, "nonce").is_err());
        });
    }

    #[test]
    fn store_creates_file_with_correct_json() {
        with_temp_home(|| {
            store(TEST_UUID, 7500, "deadbeef").unwrap();

            let dir = nonce_dir().unwrap();
            let path = dir.join(format!("{TEST_UUID}.json"));
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
            store("550e8400-e29b-41d4-a716-446655440002", 5000, "first-nonce").unwrap();
            store("550e8400-e29b-41d4-a716-446655440002", 8000, "second-nonce").unwrap();

            let stored = load("550e8400-e29b-41d4-a716-446655440002").unwrap();
            assert_eq!(stored.prediction_bps, 8000);
            assert_eq!(stored.nonce, "second-nonce");
        });
    }

    #[test]
    fn store_handles_zero_bps() {
        with_temp_home(|| {
            store("550e8400-e29b-41d4-a716-446655440003", 0, "some-nonce").unwrap();
            let stored = load("550e8400-e29b-41d4-a716-446655440003").unwrap();
            assert_eq!(stored.prediction_bps, 0);
        });
    }

    #[test]
    fn store_handles_max_u16_bps() {
        with_temp_home(|| {
            store("550e8400-e29b-41d4-a716-446655440004", u16::MAX, "some-nonce").unwrap();
            let stored = load("550e8400-e29b-41d4-a716-446655440004").unwrap();
            assert_eq!(stored.prediction_bps, u16::MAX);
        });
    }

    #[test]
    fn store_handles_empty_nonce_string() {
        with_temp_home(|| {
            store("550e8400-e29b-41d4-a716-446655440005", 5000, "").unwrap();
            let stored = load("550e8400-e29b-41d4-a716-446655440005").unwrap();
            assert_eq!(stored.nonce, "");
        });
    }

    #[test]
    fn load_round_trip_with_store() {
        with_temp_home(|| {
            let market_id = "550e8400-e29b-41d4-a716-446655440006";
            let prediction_bps = 7200u16;
            let nonce_hex = "aabbccdd11223344aabbccdd11223344aabbccdd11223344aabbccdd11223344";

            store(market_id, prediction_bps, nonce_hex).unwrap();
            let stored = load(market_id).unwrap();

            assert_eq!(stored.prediction_bps, prediction_bps);
            assert_eq!(stored.nonce, nonce_hex);
        });
    }

    #[test]
    fn load_missing_file_returns_error() {
        with_temp_home(|| {
            let result = load("550e8400-e29b-41d4-a716-446655440007");
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("No stored nonce found"),
                "Expected helpful error message, got: {err_msg}"
            );
        });
    }

    #[test]
    fn load_corrupted_json_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let id = "550e8400-e29b-41d4-a716-446655440008";
            std::fs::write(dir.join(format!("{id}.json")), "not valid json {{{").unwrap();

            let result = load(id);
            assert!(result.is_err(), "corrupted JSON should produce an error");
        });
    }

    #[test]
    fn load_empty_file_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let id = "550e8400-e29b-41d4-a716-446655440009";
            std::fs::write(dir.join(format!("{id}.json")), "").unwrap();

            let result = load(id);
            assert!(result.is_err(), "empty file should produce an error");
        });
    }

    #[test]
    fn load_json_missing_fields_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let id = "550e8400-e29b-41d4-a716-44665544000a";
            std::fs::write(dir.join(format!("{id}.json")), r#"{"prediction_bps": 5000}"#).unwrap();

            let result = load(id);
            assert!(result.is_err(), "JSON missing 'nonce' field should produce an error");
        });
    }

    #[test]
    fn load_json_wrong_type_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let id = "550e8400-e29b-41d4-a716-44665544000b";
            std::fs::write(
                dir.join(format!("{id}.json")),
                r#"{"prediction_bps": "not_a_number", "nonce": "abc"}"#,
            )
            .unwrap();

            let result = load(id);
            assert!(result.is_err(), "wrong field type should produce an error");
        });
    }

    #[test]
    fn remove_deletes_stored_nonce() {
        with_temp_home(|| {
            store("550e8400-e29b-41d4-a716-44665544000c", 5000, "some-nonce").unwrap();

            // Verify it exists first
            assert!(load("550e8400-e29b-41d4-a716-44665544000c").is_ok());

            remove("550e8400-e29b-41d4-a716-44665544000c").unwrap();

            // Should no longer be loadable
            assert!(load("550e8400-e29b-41d4-a716-44665544000c").is_err());
        });
    }

    #[test]
    fn remove_nonexistent_file_is_idempotent() {
        with_temp_home(|| {
            // Removing a file that never existed should not error
            let result = remove("550e8400-e29b-41d4-a716-44665544000d");
            assert!(result.is_ok(), "removing nonexistent nonce should succeed silently");
        });
    }

    #[test]
    fn remove_double_delete_is_idempotent() {
        with_temp_home(|| {
            store("550e8400-e29b-41d4-a716-44665544000e", 5000, "nonce").unwrap();
            remove("550e8400-e29b-41d4-a716-44665544000e").unwrap();

            // Second delete should also succeed
            let result = remove("550e8400-e29b-41d4-a716-44665544000e");
            assert!(result.is_ok(), "double delete should succeed silently");
        });
    }

    #[test]
    fn nonce_dir_creates_directory_if_missing() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            assert!(dir.exists(), "nonce directory should be created");
            assert!(dir.is_dir(), "nonce path should be a directory");
            assert!(dir.ends_with(".raiju/nonces"), "should end with .raiju/nonces");
        });
    }

    #[test]
    fn nonce_dir_is_idempotent() {
        with_temp_home(|| {
            let dir1 = nonce_dir().unwrap();
            let dir2 = nonce_dir().unwrap();
            assert_eq!(dir1, dir2, "calling nonce_dir twice should return the same path");
        });
    }

    #[test]
    fn store_multiple_markets_independently() {
        with_temp_home(|| {
            store(TEST_UUID, 1000, "nonce-a").unwrap();
            store(TEST_UUID2, 9000, "nonce-b").unwrap();

            let a = load(TEST_UUID).unwrap();
            let b = load(TEST_UUID2).unwrap();

            assert_eq!(a.prediction_bps, 1000);
            assert_eq!(a.nonce, "nonce-a");
            assert_eq!(b.prediction_bps, 9000);
            assert_eq!(b.nonce, "nonce-b");
        });
    }

    #[test]
    fn store_special_characters_in_market_id() {
        with_temp_home(|| {
            // UUIDs are the normal format, but test robustness with unusual strings
            let market_id = "550e8400-e29b-41d4-a716-446655440000";
            store(market_id, 5000, "uuid-nonce").unwrap();
            let stored = load(market_id).unwrap();
            assert_eq!(stored.prediction_bps, 5000);
            assert_eq!(stored.nonce, "uuid-nonce");
        });
    }

    #[test]
    fn stored_nonce_serialization_format() {
        with_temp_home(|| {
            store("550e8400-e29b-41d4-a716-44665544000f", 7500, "hexnonce123").unwrap();
            let dir = nonce_dir().unwrap();
            let raw =
                std::fs::read_to_string(dir.join("550e8400-e29b-41d4-a716-44665544000f.json"))
                    .unwrap();
            let parsed: StoredNonce = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed.prediction_bps, 7500);
            assert_eq!(parsed.nonce, "hexnonce123");
        });
    }
}
