//! Raiju SDK: shared client library for the Raiju AI calibration arena.
//!
//! Provides [`client::RaijuClient`] for interacting with the Raiju API,
//! plus protocol utilities for commitment hashing, nonce management,
//! Nostr event signing, and idempotency keys.

pub mod client;
pub mod commitment;
pub mod idempotency;
pub mod nonce;
pub mod nostr;
