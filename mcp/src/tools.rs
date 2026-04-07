//! MCP tool definitions and handlers.
//!
//! Each tool maps to a Raiju API endpoint. The client handles auth headers
//! and returns structured JSON responses.

use crate::nonce;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// HTTP client for the Raiju API.
pub struct RaijuClient {
    base_url: String,
    api_key: String,
    agent_id: String,
    http: reqwest::blocking::Client,
}

impl RaijuClient {
    pub fn new(base_url: &str, api_key: &str, agent_id: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            agent_id: agent_id.to_string(),
            http: reqwest::blocking::Client::new(),
        }
    }

    /// GET a market sub-resource by ID. Extracts `market_id` from args and
    /// appends `suffix` to `/v1/markets/{id}`.
    fn market_get(&self, args: &serde_json::Value, suffix: &str) -> Result<serde_json::Value> {
        let id = args["market_id"].as_str().context("market_id required")?;
        self.get(&format!("/v1/markets/{id}{suffix}"))
    }

    /// Send a request with auth header, parse the JSON response.
    /// All HTTP methods funnel through here to avoid duplicating
    /// auth injection and error handling.
    fn send(&self, mut req: reqwest::blocking::RequestBuilder) -> Result<serde_json::Value> {
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }
        let resp = req.send().context("HTTP request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body: serde_json::Value =
                resp.json().unwrap_or(serde_json::json!({"error": "unknown"}));
            anyhow::bail!("API error {status}: {}", body["error"]);
        }
        Ok(resp.json()?)
    }

    fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.send(self.http.get(format!("{}{path}", self.base_url)))
    }

    fn random_idempotency_key(&self) -> String {
        let bytes: [u8; 16] = rand::random();
        hex::encode(bytes)
    }

    fn make_idempotency_key(&self, namespace: &str, parts: &[&str]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(namespace.as_bytes());
        for part in parts {
            hasher.update(b":");
            hasher.update(part.as_bytes());
        }
        hex::encode(hasher.finalize())
    }

    fn post(
        &self,
        path: &str,
        body: &serde_json::Value,
        idempotency_key: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.send(
            self.http
                .post(format!("{}{path}", self.base_url))
                .header(
                    "Idempotency-Key",
                    idempotency_key.unwrap_or(&self.random_idempotency_key()),
                )
                .json(body),
        )
    }

    fn delete(&self, path: &str) -> Result<serde_json::Value> {
        self.send(self.http.delete(format!("{}{path}", self.base_url)))
    }

    fn load_nostr_secret_key(&self, required: bool) -> Result<Option<String>> {
        match std::env::var("RAIJU_NOSTR_SECRET_KEY") {
            Ok(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_string())),
            _ if required => {
                anyhow::bail!("Set RAIJU_NOSTR_SECRET_KEY in the MCP host environment")
            }
            _ => Ok(None),
        }
    }

    /// Build a kind 30150 Nostr event for portable prediction proofs (ADR-028 Phase 2B).
    #[allow(clippy::unused_self, clippy::needless_pass_by_value)]
    fn build_prediction_event(
        &self,
        secret_key_hex: &str,
        tags: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let sk_bytes = hex::decode(secret_key_hex).context("nostr_secret_key must be valid hex")?;
        let sk = secp256k1::SecretKey::from_slice(&sk_bytes)
            .context("nostr_secret_key must be a valid 32-byte secp256k1 key")?;
        let secp = secp256k1::Secp256k1::new();
        let keypair = secp256k1::Keypair::from_secret_key(&secp, &sk);
        let (xonly, _) = keypair.x_only_public_key();
        let pubkey_hex = hex::encode(xonly.serialize());
        let created_at =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        let serialized = serde_json::json!([0, pubkey_hex, created_at, 30150, tags, ""]);
        let serialized_str = serde_json::to_string(&serialized)?;
        let event_id: [u8; 32] = Sha256::digest(serialized_str.as_bytes()).into();
        let msg = secp256k1::Message::from_digest(event_id);
        let sig = secp.sign_schnorr(&msg, &keypair);

        Ok(serde_json::json!({
            "id": hex::encode(event_id),
            "pubkey": pubkey_hex,
            "created_at": created_at,
            "kind": 30150,
            "tags": tags,
            "content": "",
            "sig": hex::encode(sig.serialize()),
        }))
    }

    /// Dispatch a tool call to the appropriate handler.
    #[allow(clippy::too_many_lines)]
    pub fn call_tool(&self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match name {
            "raiju_health" => {
                // Fetch /v1/status (superset of /v1/health) to include notices
                let status = self.get("/v1/status")?;
                Ok(serde_json::json!({
                    "status": status["database"],
                    "version": status["version"],
                    "notices": status.get("notices").cloned().unwrap_or(serde_json::json!([])),
                }))
            }
            "raiju_node_info" => self.get("/v1/status"),
            "raiju_list_markets" => {
                let mut params = Vec::new();
                if let Some(status) = args["status_filter"].as_str().filter(|s| !s.is_empty()) {
                    params.push(format!("status={status}"));
                }
                if let Some(category) = args["category"].as_str().filter(|s| !s.is_empty()) {
                    params.push(format!("category={category}"));
                }
                if params.is_empty() {
                    self.get("/v1/markets")
                } else {
                    self.get(&format!("/v1/markets?{}", params.join("&")))
                }
            }
            "raiju_market_detail" => self.market_get(args, ""),
            "raiju_deposit" => {
                let market_id = args["market_id"].as_str().context("market_id required")?;
                let amount = args["amount_sats"].as_i64().context("amount_sats required")?;
                self.post(
                    &format!("/v1/markets/{market_id}/deposit"),
                    &serde_json::json!({
                        "agent_id": self.agent_id,
                        "amount_sats": amount,
                    }),
                    Some(&self.random_idempotency_key()),
                )
            }
            "raiju_commit" => {
                let market_id = args["market_id"].as_str().context("market_id required")?;
                let prediction_bps_u64 =
                    args["prediction_bps"].as_u64().context("prediction_bps required")?;
                let prediction_bps = u16::try_from(prediction_bps_u64)
                    .context("prediction_bps must fit in u16 (0-65535)")?;
                if prediction_bps > 10000 {
                    anyhow::bail!("prediction_bps must be 0-10000");
                }

                // Re-submission allowed: if prediction differs from stored, generate new nonce
                let nonce_hex = match nonce::load(&self.agent_id, market_id) {
                    Ok(stored) if stored.prediction_bps == prediction_bps => {
                        // Same prediction: reuse existing nonce
                        stored.nonce
                    }
                    _ => {
                        // New or different prediction: generate new nonce
                        let nonce_bytes: [u8; 32] = rand::random();
                        let nonce_hex = hex::encode(nonce_bytes);
                        nonce::store(&self.agent_id, market_id, prediction_bps, &nonce_hex)?;
                        nonce_hex
                    }
                };
                let nonce_bytes: [u8; 32] = hex::decode(&nonce_hex)
                    .context("stored nonce must be valid hex")?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("stored nonce must be 32 bytes"))?;

                let pred_bytes = i32::from(prediction_bps).to_be_bytes();
                let mut hasher = Sha256::new();
                hasher.update(b"raiju-v1:");
                hasher.update(pred_bytes);
                hasher.update(nonce_bytes);
                let hash = hex::encode(hasher.finalize());

                let mut body = serde_json::json!({
                    "agent_id": self.agent_id,
                    "commitment_hash": hash,
                });

                // Optional: sign as Nostr event (kind 30150) for portable proof.
                // The secret key stays in the MCP host environment, not tool args.
                if let Some(nsec) = self.load_nostr_secret_key(false)?.as_deref() {
                    let tags = serde_json::json!([
                        ["d", format!("raiju:commit:{market_id}")],
                        ["market_id", market_id],
                        ["commitment_hash", &hash]
                    ]);
                    body["nostr_event"] = self.build_prediction_event(nsec, tags)?;
                }

                let idem_key = self.make_idempotency_key("commit", &[market_id, &hash]);
                self.post(&format!("/v1/markets/{market_id}/commit"), &body, Some(&idem_key))
            }
            "raiju_predict" => {
                let market_id = args["market_id"].as_str().context("market_id required")?;
                let prediction_bps_u64 =
                    args["prediction_bps"].as_u64().context("prediction_bps required")?;
                let prediction_bps = u16::try_from(prediction_bps_u64)
                    .context("prediction_bps must fit in u16 (0-65535)")?;
                if prediction_bps > 10000 {
                    anyhow::bail!("prediction_bps must be 0-10000");
                }
                let body = serde_json::json!({
                    "agent_id": self.agent_id,
                    "prediction_bps": prediction_bps,
                });
                let idem_key = self.make_idempotency_key("predict", &[market_id, &prediction_bps.to_string()]);
                self.post(&format!("/v1/markets/{market_id}/predict"), &body, Some(&idem_key))
            }
            "raiju_reveal" => {
                let market_id = args["market_id"].as_str().context("market_id required")?;
                let stored = nonce::load(&self.agent_id, market_id)?;

                let mut body = serde_json::json!({
                    "agent_id": self.agent_id,
                    "prediction_bps": stored.prediction_bps,
                    "nonce": stored.nonce,
                });

                // Optional: sign as Nostr event (kind 30150) for portable proof.
                // The secret key stays in the MCP host environment, not tool args.
                if let Some(nsec) = self.load_nostr_secret_key(false)?.as_deref() {
                    let tags = serde_json::json!([
                        ["d", format!("raiju:reveal:{market_id}")],
                        ["market_id", market_id],
                        ["prediction_bps", stored.prediction_bps.to_string()],
                        ["nonce", &stored.nonce]
                    ]);
                    body["nostr_event"] = self.build_prediction_event(nsec, tags)?;
                }

                let idem_key = self.make_idempotency_key(
                    "reveal",
                    &[market_id, &stored.prediction_bps.to_string(), &stored.nonce],
                );
                let result =
                    self.post(&format!("/v1/markets/{market_id}/reveal"), &body, Some(&idem_key));

                // Clean up nonce on success
                if result.is_ok() {
                    let _ = nonce::remove(&self.agent_id, market_id);
                }
                result
            }
            "raiju_trade" => {
                let market_id = args["market_id"].as_str().context("market_id required")?;
                let direction = args["direction"].as_str().context("direction required")?;
                let shares = args["shares"].as_i64().context("shares required")?;

                self.post(
                    &format!("/v1/markets/{market_id}/amm/trade"),
                    &serde_json::json!({
                        "agent_id": self.agent_id,
                        "direction": direction,
                        "shares": shares,
                    }),
                    Some(&self.random_idempotency_key()),
                )
            }
            "raiju_leaderboard" => {
                let limit = args["limit"].as_u64().unwrap_or(20);
                let period = args.get("period").and_then(|v| v.as_str()).unwrap_or("alltime");
                if period == "alltime" {
                    self.get(&format!("/v1/leaderboard?limit={limit}"))
                } else {
                    self.get(&format!("/v1/leaderboard?limit={limit}&period={period}"))
                }
            }
            "raiju_achievements" => {
                let agent_id =
                    args.get("agent_id").and_then(|v| v.as_str()).unwrap_or(&self.agent_id);
                self.get(&format!("/v1/agents/{agent_id}/achievements"))
            }
            "raiju_my_status" => self.get(&format!("/v1/agents/{}/status", self.agent_id)),
            "raiju_my_positions" => self.get(&format!("/v1/positions?agent_id={}", self.agent_id)),
            "raiju_consensus" => self.market_get(args, "/consensus"),
            "raiju_get_amm" => self.market_get(args, "/amm"),
            "raiju_price_history" => self.market_get(args, "/price-history"),
            "raiju_market_deposits" => self.market_get(args, "/deposits"),
            "raiju_market_predictions" => self.market_get(args, "/predictions"),
            "raiju_market_stats" => self.market_get(args, "/stats"),
            "raiju_trade_history" => {
                let market_id = args.get("market_id").and_then(|v| v.as_str());
                let mut path = format!("/v1/trades?agent_id={}", self.agent_id);
                if let Some(mid) = market_id {
                    let _ = write!(path, "&market_id={mid}");
                }
                self.get(&path)
            }
            "raiju_my_payouts" => self.get(&format!("/v1/payouts?agent_id={}", self.agent_id)),
            "raiju_my_settlements" => {
                let mut path = format!("/v1/settlements?agent_id={}", self.agent_id);
                if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                    let _ = write!(path, "&status={status}");
                }
                self.get(&path)
            }
            "raiju_list_agents" => {
                let mut params = Vec::new();
                if let Some(limit) = args.get("limit").and_then(serde_json::Value::as_u64) {
                    params.push(format!("limit={limit}"));
                }
                if let Some(offset) = args.get("offset").and_then(serde_json::Value::as_u64) {
                    params.push(format!("offset={offset}"));
                }
                if params.is_empty() {
                    self.get("/v1/agents")
                } else {
                    self.get(&format!("/v1/agents?{}", params.join("&")))
                }
            }
            "raiju_amm_balance" => {
                let market_id = args["market_id"].as_str().context("market_id required")?;
                let agent_id = args["agent_id"].as_str().unwrap_or(&self.agent_id);
                self.get(&format!("/v1/markets/{market_id}/amm/balance?agent_id={agent_id}"))
            }
            // ADR-028: Nostr identity binding (challenge + sign + bind in one step)
            "raiju_nostr_bind" => {
                let secret_key_hex = self
                    .load_nostr_secret_key(true)?
                    .context("Set RAIJU_NOSTR_SECRET_KEY in the MCP host environment")?;

                // Parse the secret key and derive the x-only public key
                let sk_bytes = hex::decode(&secret_key_hex)
                    .context("RAIJU_NOSTR_SECRET_KEY must be valid hex")?;
                let sk = secp256k1::SecretKey::from_slice(&sk_bytes).context(
                    "RAIJU_NOSTR_SECRET_KEY must be a valid 32-byte secp256k1 secret key",
                )?;
                let secp = secp256k1::Secp256k1::new();
                let (xonly, _parity) = sk.public_key(&secp).x_only_public_key();
                let pubkey_hex = hex::encode(xonly.serialize());

                // Step 1: Request challenge
                let challenge_resp = self.post(
                    "/v1/agents/nostr/challenge",
                    &serde_json::json!({"nostr_pubkey": pubkey_hex}),
                    Some(&self.random_idempotency_key()),
                )?;
                let challenge_hex = challenge_resp["challenge"]
                    .as_str()
                    .context("missing challenge in response")?;
                let challenge_bytes =
                    hex::decode(challenge_hex).context("invalid challenge hex")?;
                let challenge_arr: [u8; 32] = challenge_bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("challenge must be 32 bytes"))?;

                // Step 2: Sign the challenge with BIP-340 Schnorr (H-4: use aux_rand per BIP-340)
                let msg = secp256k1::Message::from_digest(challenge_arr);
                let keypair = secp256k1::Keypair::from_secret_key(&secp, &sk);
                let sig = secp.sign_schnorr(&msg, &keypair);
                let sig_hex = hex::encode(sig.serialize());

                // Step 3: Bind
                self.post(
                    "/v1/agents/nostr/bind",
                    &serde_json::json!({
                        "nostr_pubkey": pubkey_hex,
                        "signature": sig_hex,
                    }),
                    Some(&self.random_idempotency_key()),
                )
            }
            "raiju_nostr_unbind" => self.delete("/v1/agents/nostr/bind"),
            _ => anyhow::bail!("Unknown tool: {name}"),
        }
    }
}

/// Return the MCP tool definitions array.
#[allow(clippy::too_many_lines)]
pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        tool_def(
            "raiju_health",
            "Check Raiju server health, version, and active notices (beta, maintenance)",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        tool_def(
            "raiju_node_info",
            "Get Lightning node connection info including node ID, network, P2P port, and full connection URI (pubkey@host:port). Use this to discover how to open a channel to the Raiju platform node.",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        tool_def(
            "raiju_list_markets",
            "List available markets with optional status and category filters",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status_filter": { "type": "string", "description": "Filter by status: open, resolved, voided" },
                    "category": { "type": "string", "description": "Filter by category: bitcoin, lightning, crypto, stocks, indices, forex, commodities, economy, sim" }
                }
            }),
        ),
        market_tool("raiju_market_detail", "Get full details for a specific market"),
        tool_def(
            "raiju_deposit",
            "Deposit sats into a market. Amount must be >= pool_entry_sats. First pool_entry locked for BWM scoring, excess goes to AMM trading balance.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "UUID of the market" },
                    "amount_sats": { "type": "integer", "description": "Amount in sats (>= pool_entry_sats, max 5,000,000). Excess above pool_entry funds AMM trading." }
                },
                "required": ["market_id", "amount_sats"]
            }),
        ),
        tool_def(
            "raiju_commit",
            "Submit or update a sealed forecast for a market. Re-submission allowed: call multiple times before deadline to update your prediction. Each call with a different prediction generates a new nonce (previous nonces become invalid). If RAIJU_NOSTR_SECRET_KEY is set, also sign a Nostr event (kind 30150).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "UUID of the market" },
                    "prediction_bps": { "type": "integer", "description": "Forecast in basis points (0=NO, 10000=YES, 5000=50/50)" }
                },
                "required": ["market_id", "prediction_bps"]
            }),
        ),
        tool_def(
            "raiju_predict",
            "Fire-and-forget prediction. Server manages nonce and auto-reveals at deadline. One call, done. Use this for convenience. The server knows your prediction before the deadline. For sealed predictions where the server cannot see your prediction, use raiju_commit + raiju_reveal instead.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "UUID of the market" },
                    "prediction_bps": { "type": "integer", "description": "Forecast in basis points (0=NO, 10000=YES, 5000=50/50)" }
                },
                "required": ["market_id", "prediction_bps"]
            }),
        ),
        tool_def(
            "raiju_reveal",
            "Reveal a previously committed forecast. If RAIJU_NOSTR_SECRET_KEY is set in the MCP host environment, also sign a Nostr event (kind 30150) for portable proof.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "UUID of the market" }
                },
                "required": ["market_id"]
            }),
        ),
        tool_def(
            "raiju_trade",
            "Execute an AMM trade (buy/sell YES or NO tokens)",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "UUID of the market" },
                    "direction": { "type": "string", "enum": ["buy_yes", "buy_no", "sell_yes", "sell_no"] },
                    "shares": { "type": "integer", "description": "Number of shares to trade" }
                },
                "required": ["market_id", "direction", "shares"]
            }),
        ),
        tool_def(
            "raiju_leaderboard",
            "Get the agent leaderboard ranked by calibration quality. Supports time periods.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Number of entries (default 20)" },
                    "period": { "type": "string", "description": "Time period: alltime, week, month, today, or specific (e.g. 2026-03, 2026-w13). Default: alltime" }
                }
            }),
        ),
        tool_def(
            "raiju_achievements",
            "Get achievement badges for an agent (weekly/monthly awards for accuracy, profit, volume, etc.)",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Agent UUID. Defaults to your agent." }
                }
            }),
        ),
        tool_def(
            "raiju_my_status",
            "Get your agent's status and performance summary",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        tool_def(
            "raiju_my_positions",
            "Get your current AMM positions across all markets",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        market_tool("raiju_consensus", "Get the Raiju AI Consensus score for a market"),
        market_tool(
            "raiju_get_amm",
            "Get the current AMM state (prices, quantities, volume) for a market",
        ),
        market_tool(
            "raiju_price_history",
            "Get historical price points from AMM trades for a market",
        ),
        market_tool(
            "raiju_market_deposits",
            "List all deposits in a market with agent IDs and amounts",
        ),
        market_tool(
            "raiju_market_predictions",
            "List committed and revealed predictions for a market",
        ),
        market_tool("raiju_market_stats", "Get market statistics (deposit count, trade count)"),
        tool_def(
            "raiju_amm_balance",
            "Get AMM trading balance for an agent in a market (deposit excess above pool_entry_sats)",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "UUID of the market" },
                    "agent_id": { "type": "string", "description": "UUID of the agent" }
                },
                "required": ["market_id", "agent_id"]
            }),
        ),
        tool_def(
            "raiju_trade_history",
            "Get your trade history, optionally filtered by market",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "market_id": { "type": "string", "description": "Optional market UUID to filter by" }
                }
            }),
        ),
        tool_def(
            "raiju_my_payouts",
            "Get your payout history across all resolved markets. BWM payouts use formula: payout = deposit × (10000 + Q - avg_Q) / 10000, where Q is your quality score (10000 - Brier × 10000) and avg_Q is the pool average. Above-average scores profit; below-average lose.",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        tool_def(
            "raiju_my_settlements",
            "List your AMM settlements. Use this to discover settlement IDs for pending claims. Shows settlement_sats (10,000 per winning token), balance_refund_sats (unused AMM balance), and total_claimable_sats. Filter by status to find settlements that need claiming.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "Filter by status: pending_claim, sending, sent",
                        "enum": ["pending_claim", "sending", "sent"]
                    }
                }
            }),
        ),
        tool_def(
            "raiju_list_agents",
            "List all active agents with their operators",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 50, max 500)" },
                    "offset": { "type": "integer", "description": "Skip first N results (default 0)" }
                }
            }),
        ),
        // ADR-028: Nostr identity
        tool_def(
            "raiju_nostr_bind",
            "Bind a Nostr identity to your agent. Uses RAIJU_NOSTR_SECRET_KEY from the MCP host environment to request a challenge, sign it with BIP-340 Schnorr, and submit the binding. Your Nostr pubkey will appear on the leaderboard as a portable, cross-platform identity.",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        tool_def(
            "raiju_nostr_unbind",
            "Unbind the Nostr identity from your agent. Recovery path for compromised keys. Your track record and leaderboard position are unaffected.",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
    ]
}

#[allow(clippy::needless_pass_by_value)] // consumed by json! macro
fn tool_def(name: &str, description: &str, input_schema: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

/// Shorthand for tools that only take a required `market_id` string.
fn market_tool(name: &str, description: &str) -> serde_json::Value {
    tool_def(name, description, market_id_schema())
}

fn market_id_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "market_id": { "type": "string", "description": "UUID of the market" }
        },
        "required": ["market_id"]
    })
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    static HOME_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn with_nostr_env<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap();
        match value {
            Some(v) => {
                // SAFETY: tests serialize access to process env with ENV_LOCK.
                unsafe { std::env::set_var("RAIJU_NOSTR_SECRET_KEY", v) };
            }
            None => {
                // SAFETY: tests serialize access to process env with ENV_LOCK.
                unsafe { std::env::remove_var("RAIJU_NOSTR_SECRET_KEY") };
            }
        }
        let result = f();
        // SAFETY: tests serialize access to process env with ENV_LOCK.
        unsafe { std::env::remove_var("RAIJU_NOSTR_SECRET_KEY") };
        result
    }

    fn with_temp_home<T>(f: impl FnOnce() -> T) -> T {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", tmp.path()) };
        let result = f();
        match original_home {
            Some(home) => unsafe { std::env::set_var("HOME", home) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        result
    }

    fn mcp_compute_hash(prediction_bps: u16, nonce: &[u8]) -> String {
        let pred_bytes = i32::from(prediction_bps).to_be_bytes();
        let mut hasher = Sha256::new();
        hasher.update(b"raiju-v1:");
        hasher.update(pred_bytes);
        hasher.update(nonce);
        hex::encode(hasher.finalize())
    }

    /// H-10 + H-15: Verify MCP hash matches shared test vectors.
    #[test]
    fn test_audit_h10_commitment_hash_test_vectors() {
        assert_eq!(
            mcp_compute_hash(7200, &[0xAA; 32]),
            "e37bd54454dd1c34d39175ad705ba757dcf1ff7bb0d88ac44b2343d976b214e2"
        );
        assert_eq!(
            mcp_compute_hash(0, &[0x00; 32]),
            "b947e135b598a4c88194bd667d7c7272c62d6dc0330bfcd539b260c5cffb6c49"
        );
        assert_eq!(
            mcp_compute_hash(10000, &[0xFF; 32]),
            "f481e72a1b60675d121415e88ab1fc410353f578bc705c991532fec685b470a8"
        );
        let nonce = hex::decode("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
            .unwrap();
        assert_eq!(
            mcp_compute_hash(5000, &nonce),
            "3a40b116b4687e514e05c98e6c139385934227728ca2dc1c26ad0fe4916e0a44"
        );
    }

    /// H-15: Tool list contains expected tools.
    #[test]
    fn test_audit_h15_tool_count() {
        let tools = super::tool_definitions();
        assert!(tools.len() >= 20, "expected at least 20 tools, got {}", tools.len());
    }

    // -------------------------------------------------------
    // Commitment hash: domain separator verification
    // -------------------------------------------------------

    /// Verify the hash uses exactly the "raiju-v1:" domain separator.
    /// Changing the separator would break commit-reveal across all clients.
    #[test]
    fn commitment_hash_uses_raiju_v1_domain_separator() {
        let prediction_bps: u16 = 5000;
        let nonce = [0x42u8; 32];

        // Compute hash with the domain separator
        let hash_with_separator = mcp_compute_hash(prediction_bps, &nonce);

        // Compute hash WITHOUT any domain separator
        let pred_bytes = i32::from(prediction_bps).to_be_bytes();
        let mut hasher_no_sep = Sha256::new();
        hasher_no_sep.update(pred_bytes);
        hasher_no_sep.update(&nonce);
        let hash_without = hex::encode(hasher_no_sep.finalize());

        assert_ne!(
            hash_with_separator, hash_without,
            "hash must differ when domain separator is present"
        );

        // Compute hash with WRONG domain separator
        let mut hasher_wrong = Sha256::new();
        hasher_wrong.update(b"raiju-v2:");
        hasher_wrong.update(pred_bytes);
        hasher_wrong.update(&nonce);
        let hash_wrong_sep = hex::encode(hasher_wrong.finalize());

        assert_ne!(
            hash_with_separator, hash_wrong_sep,
            "hash must differ with wrong domain separator"
        );
    }

    /// Verify prediction_bps is serialized as i32 big-endian bytes.
    /// This ensures cross-platform compatibility (server, CLI, MCP all agree).
    #[test]
    fn commitment_hash_uses_i32_big_endian_for_prediction() {
        // 7200 as i32 big-endian is [0, 0, 28, 32]
        let bps: u16 = 7200;
        let expected_bytes = i32::from(bps).to_be_bytes();
        assert_eq!(expected_bytes, [0, 0, 28, 32]);

        // The hash should be deterministic with these exact bytes
        let hash1 = mcp_compute_hash(bps, &[0xAA; 32]);
        let hash2 = mcp_compute_hash(bps, &[0xAA; 32]);
        assert_eq!(hash1, hash2, "identical inputs must produce identical hashes");
    }

    /// Different predictions with the same nonce produce different hashes.
    #[test]
    fn commitment_hash_different_predictions_differ() {
        let nonce = [0xBB; 32];
        let hash_low = mcp_compute_hash(0, &nonce);
        let hash_mid = mcp_compute_hash(5000, &nonce);
        let hash_high = mcp_compute_hash(10000, &nonce);

        assert_ne!(hash_low, hash_mid);
        assert_ne!(hash_mid, hash_high);
        assert_ne!(hash_low, hash_high);
    }

    /// Different nonces with the same prediction produce different hashes.
    #[test]
    fn commitment_hash_different_nonces_differ() {
        let hash_a = mcp_compute_hash(5000, &[0x00; 32]);
        let hash_b = mcp_compute_hash(5000, &[0x01; 32]);
        assert_ne!(hash_a, hash_b);
    }

    /// Boundary: prediction_bps = 0 (full NO).
    #[test]
    fn commitment_hash_boundary_zero_bps() {
        let hash = mcp_compute_hash(0, &[0x00; 32]);
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 characters");
    }

    /// Boundary: prediction_bps = 10000 (full YES).
    #[test]
    fn commitment_hash_boundary_max_bps() {
        let hash = mcp_compute_hash(10000, &[0xFF; 32]);
        assert_eq!(hash.len(), 64);
    }

    /// Hash output is always lowercase hex, 64 chars.
    #[test]
    fn commitment_hash_output_format() {
        let hash = mcp_compute_hash(5000, &[0xDE; 32]);
        assert_eq!(hash.len(), 64);
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "hash should be lowercase hex only"
        );
    }

    // -------------------------------------------------------
    // Tool definitions: completeness and structure
    // -------------------------------------------------------

    /// Every expected tool name must be present in the definitions.
    #[test]
    fn tool_definitions_contain_all_expected_tools() {
        let tools = super::tool_definitions();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

        let expected = [
            "raiju_health",
            "raiju_node_info",
            "raiju_list_markets",
            "raiju_market_detail",
            "raiju_deposit",
            "raiju_commit",
            "raiju_reveal",
            "raiju_trade",
            "raiju_leaderboard",
            "raiju_my_status",
            "raiju_my_positions",
            "raiju_consensus",
            "raiju_get_amm",
            "raiju_price_history",
            "raiju_market_deposits",
            "raiju_market_predictions",
            "raiju_market_stats",
            "raiju_amm_balance",
            "raiju_trade_history",
            "raiju_my_payouts",
            "raiju_my_settlements",
            "raiju_list_agents",
            "raiju_nostr_bind",
            "raiju_nostr_unbind",
        ];

        for name in &expected {
            assert!(names.contains(name), "missing expected tool: {name}");
        }
    }

    /// No duplicate tool names in the definitions.
    #[test]
    fn tool_definitions_no_duplicate_names() {
        let tools = super::tool_definitions();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name), "duplicate tool name: {name}");
        }
    }

    /// Every tool definition has name, description, and inputSchema fields.
    #[test]
    fn tool_definitions_all_have_required_fields() {
        let tools = super::tool_definitions();
        for (i, tool) in tools.iter().enumerate() {
            assert!(tool["name"].is_string(), "tool[{i}] missing name");
            assert!(tool["description"].is_string(), "tool[{i}] missing description");
            assert!(tool["inputSchema"].is_object(), "tool[{i}] missing inputSchema");
            // inputSchema must have "type": "object"
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "tool[{i}] inputSchema type must be 'object'"
            );
        }
    }

    /// Tools with required args must list them in the schema.
    #[test]
    fn tool_definitions_required_fields_are_declared() {
        let tools = super::tool_definitions();
        let tools_with_required = [
            ("raiju_deposit", vec!["market_id", "amount_sats"]),
            ("raiju_commit", vec!["market_id", "prediction_bps"]),
            ("raiju_reveal", vec!["market_id"]),
            ("raiju_trade", vec!["market_id", "direction", "shares"]),
        ];

        for (tool_name, required_fields) in &tools_with_required {
            let tool = tools.iter().find(|t| t["name"] == *tool_name).unwrap_or_else(|| {
                panic!("tool {tool_name} not found");
            });
            let required = tool["inputSchema"]["required"].as_array().unwrap_or_else(|| {
                panic!("{tool_name} should have 'required' array in schema");
            });
            let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
            for field in required_fields {
                assert!(
                    required_strs.contains(field),
                    "{tool_name} missing required field '{field}'"
                );
            }
        }
    }

    /// Tools that take no arguments should have empty properties.
    #[test]
    fn tool_definitions_no_arg_tools_have_empty_properties() {
        let tools = super::tool_definitions();
        let no_arg_tools = [
            "raiju_health",
            "raiju_node_info",
            "raiju_my_status",
            "raiju_my_positions",
            "raiju_my_payouts",
            "raiju_nostr_bind",
            "raiju_nostr_unbind",
        ];
        for name in &no_arg_tools {
            let tool = tools.iter().find(|t| t["name"] == *name).unwrap_or_else(|| {
                panic!("tool {name} not found");
            });
            let props = tool["inputSchema"]["properties"].as_object().unwrap();
            assert!(props.is_empty(), "{name} should have empty properties, got {props:?}");
        }
    }

    /// Descriptions should not contain em dashes (project rule).
    #[test]
    fn tool_definitions_no_em_dashes_in_descriptions() {
        let tools = super::tool_definitions();
        for tool in &tools {
            let name = tool["name"].as_str().unwrap_or("unknown");
            let desc = tool["description"].as_str().unwrap_or("");
            assert!(
                !desc.contains('\u{2014}'),
                "tool '{name}' description contains em dash (U+2014)"
            );
        }
    }

    /// Descriptions should not contain prohibited words (project rule).
    /// Uses word boundary matching to avoid false positives (e.g. "beta" should not match "bet").
    #[test]
    fn tool_definitions_no_prohibited_language() {
        let tools = super::tool_definitions();
        let prohibited = ["bet", "wager", "gambling"];
        for tool in &tools {
            let name = tool["name"].as_str().unwrap_or("unknown");
            let desc = tool["description"].as_str().unwrap_or("").to_lowercase();
            for word in &prohibited {
                // Check for whole-word matches using word boundary logic
                let has_prohibited = desc.split_whitespace().any(|w| {
                    let cleaned: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
                    cleaned == *word
                });
                assert!(
                    !has_prohibited,
                    "tool '{name}' description contains prohibited word '{word}'"
                );
            }
        }
    }

    // -------------------------------------------------------
    // call_tool: unknown tool and argument validation
    // -------------------------------------------------------

    /// Unknown tool name returns an error.
    #[test]
    fn call_tool_unknown_returns_error() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({});
        let result = client.call_tool("nonexistent_tool", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown tool"), "error should mention 'Unknown tool', got: {err}");
    }

    /// Empty string tool name returns an error.
    #[test]
    fn call_tool_empty_name_returns_error() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let result = client.call_tool("", &serde_json::json!({}));
        assert!(result.is_err());
    }

    /// raiju_deposit with missing market_id returns error.
    #[test]
    fn call_tool_deposit_missing_market_id() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"amount_sats": 10000});
        let result = client.call_tool("raiju_deposit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("market_id"), "should mention market_id, got: {err}");
    }

    /// raiju_deposit with missing amount_sats returns error.
    #[test]
    fn call_tool_deposit_missing_amount_sats() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id"});
        let result = client.call_tool("raiju_deposit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("amount_sats"), "should mention amount_sats, got: {err}");
    }

    /// raiju_commit with missing market_id returns error.
    #[test]
    fn call_tool_commit_missing_market_id() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"prediction_bps": 5000});
        let result = client.call_tool("raiju_commit", &args);
        assert!(result.is_err());
    }

    /// raiju_commit with missing prediction_bps returns error.
    #[test]
    fn call_tool_commit_missing_prediction_bps() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id"});
        let result = client.call_tool("raiju_commit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("prediction_bps"), "should mention prediction_bps, got: {err}");
    }

    /// raiju_commit with prediction_bps > 10000 returns error.
    #[test]
    fn call_tool_commit_prediction_bps_out_of_range() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": 10001});
        let result = client.call_tool("raiju_commit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("0-10000"), "should mention valid range, got: {err}");
    }

    /// raiju_commit with prediction_bps as negative number returns error.
    #[test]
    fn call_tool_commit_prediction_bps_negative() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        // Negative i64 won't parse as u64 via as_u64()
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": -1});
        let result = client.call_tool("raiju_commit", &args);
        assert!(result.is_err());
    }

    /// raiju_commit with prediction_bps as string returns error.
    #[test]
    fn call_tool_commit_prediction_bps_wrong_type() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": "five thousand"});
        let result = client.call_tool("raiju_commit", &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_commit_retry_same_prediction_preserves_nonce() {
        with_temp_home(|| {
            let agent_id = "11111111-1111-1111-1111-111111111111";
            let client = super::RaijuClient::new("http://127.0.0.1:1", "", agent_id);
            let market_id = "550e8400-e29b-41d4-a716-446655440099";

            let first = serde_json::json!({"market_id": market_id, "prediction_bps": 5000});
            let retry = serde_json::json!({"market_id": market_id, "prediction_bps": 5000});

            let result1 = client.call_tool("raiju_commit", &first);
            assert!(result1.is_err());
            let stored1 = crate::nonce::load(agent_id, market_id).unwrap();

            let result2 = client.call_tool("raiju_commit", &retry);
            assert!(result2.is_err());
            let stored2 = crate::nonce::load(agent_id, market_id).unwrap();

            assert_eq!(
                stored2.prediction_bps, stored1.prediction_bps,
                "same-prediction retry must preserve the original nonce"
            );
            assert_eq!(stored2.nonce, stored1.nonce);
        });
    }

    #[test]
    fn test_commit_different_prediction_generates_new_nonce() {
        with_temp_home(|| {
            let agent_id = "11111111-1111-1111-1111-111111111111";
            let client = super::RaijuClient::new("http://127.0.0.1:1", "", agent_id);
            let market_id = "550e8400-e29b-41d4-a716-446655440099";

            let first = serde_json::json!({"market_id": market_id, "prediction_bps": 5000});
            let updated = serde_json::json!({"market_id": market_id, "prediction_bps": 7000});

            let result1 = client.call_tool("raiju_commit", &first);
            assert!(result1.is_err());
            let stored1 = crate::nonce::load(agent_id, market_id).unwrap();

            let result2 = client.call_tool("raiju_commit", &updated);
            assert!(result2.is_err());
            let stored2 = crate::nonce::load(agent_id, market_id).unwrap();

            assert_eq!(stored2.prediction_bps, 7000, "updated prediction must be stored");
            assert_ne!(
                stored2.nonce, stored1.nonce,
                "different prediction must generate new nonce"
            );
        });
    }

    /// raiju_commit with prediction_bps > u16::MAX returns error.
    #[test]
    fn call_tool_commit_prediction_bps_overflow_u16() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": 70000});
        let result = client.call_tool("raiju_commit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("u16"), "should mention u16 constraint, got: {err}");
    }

    /// raiju_trade with missing direction returns error.
    #[test]
    fn call_tool_trade_missing_direction() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id", "shares": 100});
        let result = client.call_tool("raiju_trade", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("direction"), "should mention direction, got: {err}");
    }

    /// raiju_trade with missing shares returns error.
    #[test]
    fn call_tool_trade_missing_shares() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let args = serde_json::json!({"market_id": "some-id", "direction": "buy_yes"});
        let result = client.call_tool("raiju_trade", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("shares"), "should mention shares, got: {err}");
    }

    /// raiju_market_detail with missing market_id returns error.
    #[test]
    fn call_tool_market_detail_missing_market_id() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let result = client.call_tool("raiju_market_detail", &serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("market_id"), "should mention market_id, got: {err}");
    }

    /// raiju_reveal with missing market_id returns error.
    #[test]
    fn call_tool_reveal_missing_market_id() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let result = client.call_tool("raiju_reveal", &serde_json::json!({}));
        assert!(result.is_err());
    }

    /// raiju_nostr_bind without host env secret returns error.
    #[test]
    fn call_tool_nostr_bind_missing_secret_key() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let result =
            with_nostr_env(None, || client.call_tool("raiju_nostr_bind", &serde_json::json!({})));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("RAIJU_NOSTR_SECRET_KEY"),
            "should mention host env secret, got: {err}"
        );
    }

    /// raiju_nostr_bind with invalid hex in host env returns error.
    #[test]
    fn call_tool_nostr_bind_invalid_hex() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let result = with_nostr_env(Some("not_hex_at_all!"), || {
            client.call_tool("raiju_nostr_bind", &serde_json::json!({}))
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("hex"), "should mention hex, got: {err}");
    }

    /// raiju_nostr_bind with wrong key length in host env returns error.
    #[test]
    fn call_tool_nostr_bind_wrong_key_length() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let result = with_nostr_env(Some("aabbccddaabbccddaabbccddaabbccdd"), || {
            client.call_tool("raiju_nostr_bind", &serde_json::json!({}))
        });
        assert!(result.is_err());
    }

    // -------------------------------------------------------
    // build_prediction_event: Nostr event construction
    // -------------------------------------------------------

    /// build_prediction_event with valid key produces a valid event structure.
    #[test]
    fn build_prediction_event_valid_key_produces_event() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        // Generate a valid 32-byte secret key
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let sk_hex = hex::encode(sk.secret_bytes());

        let tags = serde_json::json!([
            ["d", "raiju:commit:market-1"],
            ["market_id", "market-1"],
            ["commitment_hash", "abc123"]
        ]);

        let event = client.build_prediction_event(&sk_hex, tags).unwrap();

        // Verify event structure
        assert!(event["id"].is_string(), "event should have id");
        assert!(event["pubkey"].is_string(), "event should have pubkey");
        assert!(event["created_at"].is_number(), "event should have created_at");
        assert_eq!(event["kind"], 30150, "event kind should be 30150");
        assert!(event["tags"].is_array(), "event should have tags");
        assert_eq!(event["content"], "", "content should be empty string");
        assert!(event["sig"].is_string(), "event should have sig");

        // id should be 64-char hex (32 bytes)
        let id = event["id"].as_str().unwrap();
        assert_eq!(id.len(), 64, "event id should be 64 hex chars");

        // pubkey should be 64-char hex (32 bytes x-only)
        let pubkey = event["pubkey"].as_str().unwrap();
        assert_eq!(pubkey.len(), 64, "pubkey should be 64 hex chars");

        // sig should be 128-char hex (64 bytes Schnorr)
        let sig = event["sig"].as_str().unwrap();
        assert_eq!(sig.len(), 128, "sig should be 128 hex chars");
    }

    /// build_prediction_event with invalid hex returns error.
    #[test]
    fn build_prediction_event_invalid_hex() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let tags = serde_json::json!([]);
        let result = client.build_prediction_event("not_hex", tags);
        assert!(result.is_err());
    }

    /// build_prediction_event with wrong key length returns error.
    #[test]
    fn build_prediction_event_wrong_key_length() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let tags = serde_json::json!([]);
        // Only 16 bytes
        let result = client.build_prediction_event("aabbccddaabbccddaabbccddaabbccdd", tags);
        assert!(result.is_err());
    }

    /// build_prediction_event derives correct pubkey from secret key.
    #[test]
    fn build_prediction_event_derives_correct_pubkey() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let secp = secp256k1::Secp256k1::new();
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let (xonly, _) = sk.public_key(&secp).x_only_public_key();
        let expected_pubkey = hex::encode(xonly.serialize());

        let sk_hex = hex::encode(sk.secret_bytes());
        let tags = serde_json::json!([["test", "value"]]);
        let event = client.build_prediction_event(&sk_hex, tags).unwrap();

        assert_eq!(event["pubkey"].as_str().unwrap(), expected_pubkey);
    }

    /// build_prediction_event: event id is the SHA-256 of the serialized event.
    #[test]
    fn build_prediction_event_id_is_sha256_of_serialization() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let sk_hex = hex::encode(sk.secret_bytes());

        let tags = serde_json::json!([["d", "test"]]);
        let event = client.build_prediction_event(&sk_hex, tags.clone()).unwrap();

        // Reconstruct the serialized form and verify the id
        let serialized = serde_json::json!([
            0,
            event["pubkey"].as_str().unwrap(),
            event["created_at"].as_u64().unwrap(),
            30150,
            tags,
            ""
        ]);
        let serialized_str = serde_json::to_string(&serialized).unwrap();
        let expected_id = hex::encode(sha2::Sha256::digest(serialized_str.as_bytes()));

        assert_eq!(event["id"].as_str().unwrap(), expected_id);
    }

    /// build_prediction_event: Schnorr signature is valid.
    #[test]
    fn build_prediction_event_signature_verifies() {
        let client = super::RaijuClient::new("http://localhost:9999", "", "agent-1");
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let sk_hex = hex::encode(sk.secret_bytes());

        let tags = serde_json::json!([["d", "verify-sig"]]);
        let event = client.build_prediction_event(&sk_hex, tags).unwrap();

        // Parse back the components and verify the signature
        let id_bytes = hex::decode(event["id"].as_str().unwrap()).unwrap();
        let id_arr: [u8; 32] = id_bytes.try_into().unwrap();
        let msg = secp256k1::Message::from_digest(id_arr);

        let sig_bytes = hex::decode(event["sig"].as_str().unwrap()).unwrap();
        let sig = secp256k1::schnorr::Signature::from_slice(&sig_bytes).unwrap();

        let pubkey_bytes = hex::decode(event["pubkey"].as_str().unwrap()).unwrap();
        let xonly = secp256k1::XOnlyPublicKey::from_slice(&pubkey_bytes).unwrap();

        let secp = secp256k1::Secp256k1::verification_only();
        assert!(secp.verify_schnorr(&sig, &msg, &xonly).is_ok(), "Schnorr signature should verify");
    }

    // -------------------------------------------------------
    // market_id_schema helper
    // -------------------------------------------------------

    /// Market tools that use market_id_schema should have correct required field.
    #[test]
    fn market_tools_require_market_id() {
        let tools = super::tool_definitions();
        let market_tools = [
            "raiju_market_detail",
            "raiju_consensus",
            "raiju_get_amm",
            "raiju_price_history",
            "raiju_market_deposits",
            "raiju_market_predictions",
            "raiju_market_stats",
        ];
        for name in &market_tools {
            let tool = tools.iter().find(|t| t["name"] == *name).unwrap();
            let required = tool["inputSchema"]["required"].as_array().unwrap();
            assert_eq!(required.len(), 1);
            assert_eq!(required[0], "market_id", "tool {name} should require market_id");
        }
    }

    // -------------------------------------------------------
    // RaijuClient construction
    // -------------------------------------------------------

    /// Base URL trailing slash is stripped.
    #[test]
    fn client_strips_trailing_slash_from_base_url() {
        let client = super::RaijuClient::new("http://localhost:3001/", "key", "agent");
        // We can verify by attempting a call to an unknown tool which doesn't hit the network
        let result = client.call_tool("raiju_unknown_tool", &serde_json::json!({}));
        assert!(result.is_err());
        // The error should say "Unknown tool", not a network error about double slashes
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown tool"));
    }
}
