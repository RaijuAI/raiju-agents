//! Unified HTTP client for the Raiju API.
//!
//! [`RaijuClient`] provides typed methods for every public API endpoint.
//! Used by both the CLI binary and the MCP server.

use crate::{commitment, idempotency, nonce, nostr};
use anyhow::{Context, Result};
use std::time::Duration;

/// HTTP client for the Raiju API.
pub struct RaijuClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::blocking::Client,
}

impl RaijuClient {
    /// Create a new client. Timeout is 30 seconds.
    pub fn new(base_url: &str, api_key: Option<&str>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(String::from),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// The base URL this client connects to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ── Core HTTP ──────────────────────────────────────────────────────

    fn send(&self, mut req: reqwest::blocking::RequestBuilder) -> Result<serde_json::Value> {
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req.send().context("HTTP request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body: serde_json::Value =
                resp.json().unwrap_or(serde_json::json!({"error": "unknown"}));
            anyhow::bail!("{status}: {}", body["error"].as_str().unwrap_or("unknown"));
        }
        Ok(resp.json()?)
    }

    /// Authenticated GET request.
    pub fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.send(self.http.get(format!("{}{path}", self.base_url)))
    }

    /// Authenticated POST request with JSON body and idempotency key.
    pub fn post(
        &self,
        path: &str,
        body: &serde_json::Value,
        idempotency_key: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.send(
            self.http
                .post(format!("{}{path}", self.base_url))
                .header("Idempotency-Key", idempotency_key.unwrap_or(&idempotency::random_key()))
                .json(body),
        )
    }

    /// Authenticated DELETE request.
    pub fn delete(&self, path: &str) -> Result<serde_json::Value> {
        self.send(self.http.delete(format!("{}{path}", self.base_url)))
    }

    /// GET /v1/events/recent - poll the server's ring buffer.
    ///
    /// Returns up to `limit` events newer than `since` (defaults to the
    /// start of the buffer when `None`), optionally filtered by market UUID
    /// and event type. Response shape:
    ///
    /// ```json
    /// {"events": [...], "next_since": 42, "latest_event_id": 42}
    /// ```
    pub fn events_recent(
        &self,
        since: Option<u64>,
        markets: Option<&str>,
        types: Option<&str>,
        limit: Option<usize>,
    ) -> Result<serde_json::Value> {
        let mut params: Vec<String> = Vec::new();
        if let Some(s) = since {
            params.push(format!("since={s}"));
        }
        if let Some(m) = markets.filter(|s| !s.is_empty()) {
            params.push(format!("markets={m}"));
        }
        if let Some(t) = types.filter(|s| !s.is_empty()) {
            params.push(format!("types={t}"));
        }
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        let path = if params.is_empty() {
            "/v1/events/recent".to_string()
        } else {
            format!("/v1/events/recent?{}", params.join("&"))
        };
        self.get(&path)
    }

    /// Open a long-lived Server-Sent Events stream.
    ///
    /// The returned [`reqwest::blocking::Response`] implements [`std::io::Read`]
    /// and can be wrapped in a `BufReader` for line-by-line SSE parsing.
    /// A separate HTTP client with no read timeout is used so keepalive
    /// idle periods do not kill the connection.
    ///
    /// The `Authorization` header is sent when an API key is configured,
    /// even though `/v1/events` is public today, to forward-compatible
    /// with future per-agent authenticated streams.
    pub fn open_sse_stream(
        &self,
        path: &str,
        last_event_id: Option<&str>,
    ) -> Result<reqwest::blocking::Response> {
        let streaming = reqwest::blocking::Client::builder()
            .timeout(None)
            .build()
            .context("failed to build streaming HTTP client")?;
        let url = format!("{}{path}", self.base_url);
        let mut req = streaming.get(&url).header("Accept", "text/event-stream");
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        if let Some(id) = last_event_id {
            req = req.header("Last-Event-ID", id);
        }
        let resp = req.send().with_context(|| format!("SSE request to {url} failed"))?;
        if !resp.status().is_success() {
            anyhow::bail!("SSE request to {url} returned {}", resp.status());
        }
        Ok(resp)
    }

    // ── Health & discovery ─────────────────────────────────────────────

    /// GET /v1/status (health, version, node info, notices).
    pub fn status(&self) -> Result<serde_json::Value> {
        self.get("/v1/status")
    }

    /// GET /v1/markets with optional status and category filters.
    pub fn list_markets(
        &self,
        status: Option<&str>,
        category: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(format!("status={s}"));
        }
        if let Some(c) = category {
            params.push(format!("category={c}"));
        }
        if params.is_empty() {
            self.get("/v1/markets")
        } else {
            self.get(&format!("/v1/markets?{}", params.join("&")))
        }
    }

    /// GET /v1/markets/{id}.
    pub fn market_detail(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}"))
    }

    /// GET /v1/agents with optional pagination.
    pub fn list_agents(
        &self,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<serde_json::Value> {
        let mut params = Vec::new();
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        if let Some(o) = offset {
            params.push(format!("offset={o}"));
        }
        if params.is_empty() {
            self.get("/v1/agents")
        } else {
            self.get(&format!("/v1/agents?{}", params.join("&")))
        }
    }

    // ── Registration ───────────────────────────────────────────────────

    /// POST /v1/operators (no auth required).
    pub fn register_operator(
        &self,
        name: &str,
        nwc_uri: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut body = serde_json::json!({"display_name": name});
        if let Some(uri) = nwc_uri {
            body["nwc_uri"] = serde_json::Value::String(uri.to_string());
        }
        // Registration uses the raw http client (no auth header)
        let resp = self
            .http
            .post(format!("{}/v1/operators", self.base_url))
            .json(&body)
            .send()
            .context("HTTP request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body: serde_json::Value =
                resp.json().unwrap_or(serde_json::json!({"error": "unknown"}));
            anyhow::bail!("{status}: {}", body["error"].as_str().unwrap_or("unknown"));
        }
        Ok(resp.json()?)
    }

    /// POST /v1/agents.
    pub fn register_agent(
        &self,
        operator_id: &str,
        name: &str,
        description: Option<&str>,
        repo_url: Option<&str>,
        nwc_uri: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut body = serde_json::json!({
            "operator_id": operator_id,
            "display_name": name,
        });
        if let Some(d) = description {
            body["description"] = serde_json::Value::String(d.to_string());
        }
        if let Some(u) = repo_url {
            body["repo_url"] = serde_json::Value::String(u.to_string());
        }
        if let Some(u) = nwc_uri {
            body["nwc_uri"] = serde_json::Value::String(u.to_string());
        }
        self.post("/v1/agents", &body, None)
    }

    // ── Wallet ─────────────────────────────────────────────────────────

    /// POST /v1/agents/{id}/wallet.
    pub fn wallet_set(&self, agent_id: &str, nwc_uri: &str) -> Result<serde_json::Value> {
        self.post(
            &format!("/v1/agents/{agent_id}/wallet"),
            &serde_json::json!({"nwc_uri": nwc_uri}),
            None,
        )
    }

    /// GET /v1/agents/{id}/wallet.
    pub fn wallet_status(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/agents/{agent_id}/wallet"))
    }

    /// DELETE /v1/agents/{id}/wallet.
    pub fn wallet_remove(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.delete(&format!("/v1/agents/{agent_id}/wallet"))
    }

    // ── Predictions ────────────────────────────────────────────────────

    /// POST /v1/markets/{id}/deposit.
    pub fn deposit(
        &self,
        market_id: &str,
        agent_id: &str,
        amount_sats: i64,
    ) -> Result<serde_json::Value> {
        self.post(
            &format!("/v1/markets/{market_id}/deposit"),
            &serde_json::json!({
                "agent_id": agent_id,
                "amount_sats": amount_sats,
                "method": "lightning",
            }),
            Some(&idempotency::random_key()),
        )
    }

    /// Sealed commit: generate/reuse nonce, compute hash, optional Nostr sign, POST.
    pub fn commit(
        &self,
        market_id: &str,
        agent_id: &str,
        prediction_bps: u16,
        nostr_secret: Option<&str>,
    ) -> Result<serde_json::Value> {
        if prediction_bps > 10000 {
            anyhow::bail!("prediction_bps must be 0-10000");
        }

        // Re-submission allowed: reuse nonce if same prediction, else generate new
        let nonce_hex = match nonce::load(agent_id, market_id) {
            Ok(stored) if stored.prediction_bps == prediction_bps => stored.nonce,
            _ => {
                let nonce_bytes: [u8; 32] = rand::random();
                let nonce_hex = hex::encode(nonce_bytes);
                nonce::store(agent_id, market_id, prediction_bps, &nonce_hex)?;
                nonce_hex
            }
        };
        let nonce_bytes: [u8; 32] = hex::decode(&nonce_hex)
            .context("stored nonce must be valid hex")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("stored nonce must be 32 bytes"))?;

        let hash = commitment::compute_hash(prediction_bps, &nonce_bytes);

        let mut body = serde_json::json!({
            "agent_id": agent_id,
            "commitment_hash": hash,
        });

        if let Some(nsec) = nostr_secret {
            let tags = serde_json::json!([
                ["d", format!("raiju:commit:{market_id}")],
                ["market_id", market_id],
                ["commitment_hash", &hash]
            ]);
            body["nostr_event"] = nostr::build_event(nsec, 30150, tags)?;
        }

        let idem_key = idempotency::deterministic_key("commit", &[market_id, &hash]);
        self.post(&format!("/v1/markets/{market_id}/commit"), &body, Some(&idem_key))
    }

    /// Reveal: load stored nonce, optional Nostr sign, POST, clean up on success.
    pub fn reveal(
        &self,
        market_id: &str,
        agent_id: &str,
        nostr_secret: Option<&str>,
    ) -> Result<serde_json::Value> {
        let stored = nonce::load(agent_id, market_id)?;

        let mut body = serde_json::json!({
            "agent_id": agent_id,
            "prediction_bps": stored.prediction_bps,
            "nonce": stored.nonce,
        });

        if let Some(nsec) = nostr_secret {
            let tags = serde_json::json!([
                ["d", format!("raiju:reveal:{market_id}")],
                ["market_id", market_id],
                ["prediction_bps", stored.prediction_bps.to_string()],
                ["nonce", &stored.nonce]
            ]);
            body["nostr_event"] = nostr::build_event(nsec, 30150, tags)?;
        }

        let idem_key = idempotency::deterministic_key(
            "reveal",
            &[market_id, &stored.prediction_bps.to_string(), &stored.nonce],
        );
        let result = self.post(&format!("/v1/markets/{market_id}/reveal"), &body, Some(&idem_key));

        if result.is_ok() {
            let _ = nonce::remove(agent_id, market_id);
        }
        result
    }

    /// Fire-and-forget prediction: POST /v1/markets/{id}/predict.
    pub fn predict(
        &self,
        market_id: &str,
        agent_id: &str,
        prediction_bps: u16,
    ) -> Result<serde_json::Value> {
        if prediction_bps > 10000 {
            anyhow::bail!("prediction_bps must be 0-10000");
        }
        let body = serde_json::json!({
            "agent_id": agent_id,
            "prediction_bps": prediction_bps,
        });
        let idem_key =
            idempotency::deterministic_key("predict", &[market_id, &prediction_bps.to_string()]);
        self.post(&format!("/v1/markets/{market_id}/predict"), &body, Some(&idem_key))
    }

    // ── AMM trading ────────────────────────────────────────────────────

    /// POST /v1/markets/{id}/amm/trade.
    pub fn trade(
        &self,
        market_id: &str,
        agent_id: &str,
        direction: &str,
        shares: i64,
    ) -> Result<serde_json::Value> {
        self.post(
            &format!("/v1/markets/{market_id}/amm/trade"),
            &serde_json::json!({
                "agent_id": agent_id,
                "direction": direction,
                "shares": shares,
            }),
            Some(&idempotency::random_key()),
        )
    }

    // ── Market data ────────────────────────────────────────────────────

    pub fn consensus(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/consensus"))
    }

    pub fn amm_state(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/amm"))
    }

    pub fn price_history(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/price-history"))
    }

    pub fn market_deposits(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/deposits"))
    }

    pub fn market_predictions(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/predictions"))
    }

    pub fn market_stats(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/stats"))
    }

    pub fn market_payouts(&self, market_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/payouts"))
    }

    pub fn amm_balance(&self, market_id: &str, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/markets/{market_id}/amm/balance?agent_id={agent_id}"))
    }

    // ── Agent data ─────────────────────────────────────────────────────

    pub fn agent_status(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/agents/{agent_id}/status"))
    }

    pub fn agent_actions(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/agents/{agent_id}/actions"))
    }

    pub fn agent_achievements(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/agents/{agent_id}/achievements"))
    }

    pub fn positions(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/positions?agent_id={agent_id}"))
    }

    pub fn trade_history(
        &self,
        agent_id: Option<&str>,
        market_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut params = Vec::new();
        if let Some(a) = agent_id {
            params.push(format!("agent_id={a}"));
        }
        if let Some(m) = market_id {
            params.push(format!("market_id={m}"));
        }
        if params.is_empty() {
            self.get("/v1/trades")
        } else {
            self.get(&format!("/v1/trades?{}", params.join("&")))
        }
    }

    // ── Payouts & settlements ──────────────────────────────────────────

    pub fn payouts(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/payouts?agent_id={agent_id}"))
    }

    pub fn payouts_by_status(&self, agent_id: &str, status: &str) -> Result<serde_json::Value> {
        self.get(&format!("/v1/payouts?agent_id={agent_id}&status={status}"))
    }

    pub fn settlements(&self, agent_id: &str, status: Option<&str>) -> Result<serde_json::Value> {
        let mut path = format!("/v1/settlements?agent_id={agent_id}");
        if let Some(s) = status {
            path.push_str(&format!("&status={s}"));
        }
        self.get(&path)
    }

    pub fn claim_payout(
        &self,
        payout_id: &str,
        agent_id: &str,
        bolt11: &str,
    ) -> Result<serde_json::Value> {
        self.post(
            &format!("/v1/payouts/{payout_id}/claim"),
            &serde_json::json!({
                "agent_id": agent_id,
                "bolt11_invoice": bolt11,
            }),
            None,
        )
    }

    pub fn claim_settlement(
        &self,
        settlement_id: &str,
        agent_id: &str,
        bolt11: &str,
    ) -> Result<serde_json::Value> {
        self.post(
            &format!("/v1/settlements/{settlement_id}/claim"),
            &serde_json::json!({
                "agent_id": agent_id,
                "bolt11_invoice": bolt11,
            }),
            None,
        )
    }

    // ── Leaderboard ────────────────────────────────────────────────────

    pub fn leaderboard(
        &self,
        limit: Option<u64>,
        period: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut params = Vec::new();
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        if let Some(p) = period.filter(|p| *p != "alltime") {
            params.push(format!("period={p}"));
        }
        if params.is_empty() {
            self.get("/v1/leaderboard")
        } else {
            self.get(&format!("/v1/leaderboard?{}", params.join("&")))
        }
    }

    // ── Nostr identity (ADR-028) ───────────────────────────────────────

    /// Full nostr bind flow: derive pubkey, request challenge, sign, bind.
    pub fn nostr_bind(&self, secret_key_hex: &str) -> Result<serde_json::Value> {
        let (pubkey_hex, keypair) = nostr::derive_pubkey(secret_key_hex)?;

        // Step 1: Request challenge
        let challenge_resp = self.post(
            "/v1/agents/nostr/challenge",
            &serde_json::json!({"nostr_pubkey": pubkey_hex}),
            Some(&idempotency::random_key()),
        )?;
        let challenge_hex =
            challenge_resp["challenge"].as_str().context("missing challenge in response")?;
        let challenge_bytes = hex::decode(challenge_hex).context("invalid challenge hex")?;
        let challenge_arr: [u8; 32] = challenge_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("challenge must be 32 bytes"))?;

        // Step 2: Sign with BIP-340 Schnorr
        let secp = secp256k1::Secp256k1::new();
        let msg = secp256k1::Message::from_digest(challenge_arr);
        let sig = secp.sign_schnorr(&msg, &keypair);

        // Step 3: Bind
        self.post(
            "/v1/agents/nostr/bind",
            &serde_json::json!({
                "nostr_pubkey": pubkey_hex,
                "signature": hex::encode(sig.serialize()),
            }),
            Some(&idempotency::random_key()),
        )
    }

    /// DELETE /v1/agents/nostr/bind.
    pub fn nostr_unbind(&self) -> Result<serde_json::Value> {
        self.delete("/v1/agents/nostr/bind")
    }

    // ── Nostr challenge/bind (manual flow for CLI) ─────────────────────

    /// POST /v1/agents/nostr/challenge.
    pub fn nostr_challenge(&self, pubkey: &str) -> Result<serde_json::Value> {
        self.post(
            "/v1/agents/nostr/challenge",
            &serde_json::json!({"nostr_pubkey": pubkey}),
            Some(&idempotency::random_key()),
        )
    }

    /// POST /v1/agents/nostr/bind (manual with pre-signed signature).
    pub fn nostr_bind_manual(&self, pubkey: &str, signature: &str) -> Result<serde_json::Value> {
        self.post(
            "/v1/agents/nostr/bind",
            &serde_json::json!({
                "nostr_pubkey": pubkey,
                "signature": signature,
            }),
            Some(&idempotency::random_key()),
        )
    }

    // ── Agent lifecycle ────────────────────────────────────────────────

    /// POST /v1/agents/{id}/deactivate.
    pub fn deactivate_agent(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.post(&format!("/v1/agents/{agent_id}/deactivate"), &serde_json::json!({}), None)
    }

    /// POST /v1/agents/{id}/reactivate.
    pub fn reactivate_agent(&self, agent_id: &str) -> Result<serde_json::Value> {
        self.post(&format!("/v1/agents/{agent_id}/reactivate"), &serde_json::json!({}), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_new_trims_trailing_slash() {
        let client = RaijuClient::new("https://raiju.ai/", None);
        assert_eq!(client.base_url, "https://raiju.ai");
    }

    #[test]
    fn test_idempotency_key_deterministic() {
        let k1 = idempotency::deterministic_key("commit", &["market-1", "hash-1"]);
        let k2 = idempotency::deterministic_key("commit", &["market-1", "hash-1"]);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_idempotency_key_varies_by_input() {
        let k1 = idempotency::deterministic_key("commit", &["market-1", "hash-1"]);
        let k2 = idempotency::deterministic_key("commit", &["market-2", "hash-1"]);
        assert_ne!(k1, k2);
    }
}
