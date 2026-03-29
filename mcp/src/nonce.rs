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

fn nonce_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".raiju").join("nonces");
    std::fs::create_dir_all(&dir).context("failed to create nonce directory")?;
    Ok(dir)
}

pub fn store(market_id: &str, prediction_bps: u16, nonce: &str) -> Result<()> {
    let path = nonce_dir()?.join(format!("{market_id}.json"));
    let data = StoredNonce { prediction_bps, nonce: nonce.to_string() };
    std::fs::write(&path, serde_json::to_string(&data)?).context("failed to store nonce")?;
    Ok(())
}

pub fn load(market_id: &str) -> Result<StoredNonce> {
    let path = nonce_dir()?.join(format!("{market_id}.json"));
    let content = std::fs::read_to_string(&path)
        .context("No stored nonce found. Did you call raiju_commit first?")?;
    let stored: StoredNonce = serde_json::from_str(&content)?;
    Ok(stored)
}

pub fn remove(market_id: &str) -> Result<()> {
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

    #[test]
    fn store_creates_file_with_correct_json() {
        with_temp_home(|| {
            store("market-abc", 7500, "deadbeef").unwrap();

            let dir = nonce_dir().unwrap();
            let path = dir.join("market-abc.json");
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
            store("market-overwrite", 5000, "first-nonce").unwrap();
            store("market-overwrite", 8000, "second-nonce").unwrap();

            let stored = load("market-overwrite").unwrap();
            assert_eq!(stored.prediction_bps, 8000);
            assert_eq!(stored.nonce, "second-nonce");
        });
    }

    #[test]
    fn store_handles_zero_bps() {
        with_temp_home(|| {
            store("market-zero", 0, "some-nonce").unwrap();
            let stored = load("market-zero").unwrap();
            assert_eq!(stored.prediction_bps, 0);
        });
    }

    #[test]
    fn store_handles_max_u16_bps() {
        with_temp_home(|| {
            store("market-max", u16::MAX, "some-nonce").unwrap();
            let stored = load("market-max").unwrap();
            assert_eq!(stored.prediction_bps, u16::MAX);
        });
    }

    #[test]
    fn store_handles_empty_nonce_string() {
        with_temp_home(|| {
            store("market-empty-nonce", 5000, "").unwrap();
            let stored = load("market-empty-nonce").unwrap();
            assert_eq!(stored.nonce, "");
        });
    }

    #[test]
    fn load_round_trip_with_store() {
        with_temp_home(|| {
            let market_id = "market-round-trip";
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
            let result = load("nonexistent-market");
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
            let path = dir.join("corrupted-market.json");
            std::fs::write(&path, "not valid json {{{").unwrap();

            let result = load("corrupted-market");
            assert!(result.is_err(), "corrupted JSON should produce an error");
        });
    }

    #[test]
    fn load_empty_file_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let path = dir.join("empty-market.json");
            std::fs::write(&path, "").unwrap();

            let result = load("empty-market");
            assert!(result.is_err(), "empty file should produce an error");
        });
    }

    #[test]
    fn load_json_missing_fields_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let path = dir.join("partial-market.json");
            // Valid JSON but missing required fields
            std::fs::write(&path, r#"{"prediction_bps": 5000}"#).unwrap();

            let result = load("partial-market");
            assert!(result.is_err(), "JSON missing 'nonce' field should produce an error");
        });
    }

    #[test]
    fn load_json_wrong_type_returns_error() {
        with_temp_home(|| {
            let dir = nonce_dir().unwrap();
            let path = dir.join("wrongtype-market.json");
            // prediction_bps is a string instead of a number
            std::fs::write(&path, r#"{"prediction_bps": "not_a_number", "nonce": "abc"}"#).unwrap();

            let result = load("wrongtype-market");
            assert!(result.is_err(), "wrong field type should produce an error");
        });
    }

    #[test]
    fn remove_deletes_stored_nonce() {
        with_temp_home(|| {
            store("market-to-delete", 5000, "some-nonce").unwrap();

            // Verify it exists first
            assert!(load("market-to-delete").is_ok());

            remove("market-to-delete").unwrap();

            // Should no longer be loadable
            assert!(load("market-to-delete").is_err());
        });
    }

    #[test]
    fn remove_nonexistent_file_is_idempotent() {
        with_temp_home(|| {
            // Removing a file that never existed should not error
            let result = remove("never-existed-market");
            assert!(result.is_ok(), "removing nonexistent nonce should succeed silently");
        });
    }

    #[test]
    fn remove_double_delete_is_idempotent() {
        with_temp_home(|| {
            store("market-double-del", 5000, "nonce").unwrap();
            remove("market-double-del").unwrap();

            // Second delete should also succeed
            let result = remove("market-double-del");
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
            store("market-A", 1000, "nonce-a").unwrap();
            store("market-B", 9000, "nonce-b").unwrap();

            let a = load("market-A").unwrap();
            let b = load("market-B").unwrap();

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
            store("format-check", 7500, "hexnonce123").unwrap();
            let dir = nonce_dir().unwrap();
            let raw = std::fs::read_to_string(dir.join("format-check.json")).unwrap();
            let parsed: StoredNonce = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed.prediction_bps, 7500);
            assert_eq!(parsed.nonce, "hexnonce123");
        });
    }
}
