//! MCP tool definitions and handlers.
//!
//! Each tool maps to a Raiju API endpoint via [`raiju::client::RaijuClient`].
//! Tool definitions describe the JSON schema for each tool's arguments.

use anyhow::{Context, Result};
use raiju::client::RaijuClient;

/// Dispatch a tool call to the appropriate handler.
#[allow(clippy::too_many_lines)]
pub fn call_tool(
    client: &RaijuClient,
    agent_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match name {
        "raiju_health" => {
            let status = client.status()?;
            Ok(serde_json::json!({
                "status": status["database"],
                "version": status["version"],
                "notices": status.get("notices").cloned().unwrap_or(serde_json::json!([])),
            }))
        }
        "raiju_node_info" => client.status(),
        "raiju_list_markets" => {
            let status = args["status_filter"].as_str().filter(|s| !s.is_empty());
            let category = args["category"].as_str().filter(|s| !s.is_empty());
            client.list_markets(status, category)
        }
        "raiju_market_detail" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.market_detail(market_id)
        }
        "raiju_deposit" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            let amount = args["amount_sats"].as_i64().context("amount_sats required")?;
            client.deposit(market_id, agent_id, amount)
        }
        "raiju_wallet_set" => {
            let nwc_uri = args["nwc_uri"].as_str().context("nwc_uri required")?;
            client.wallet_set(agent_id, nwc_uri)
        }
        "raiju_wallet_remove" => client.wallet_remove(agent_id),
        "raiju_wallet_status" => client.wallet_status(agent_id),
        "raiju_commit" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            let prediction_bps_u64 =
                args["prediction_bps"].as_u64().context("prediction_bps required")?;
            let prediction_bps = u16::try_from(prediction_bps_u64)
                .context("prediction_bps must fit in u16 (0-65535)")?;
            let nostr_secret = raiju::nostr::load_secret_key(None, false)?;
            client.commit(market_id, agent_id, prediction_bps, nostr_secret.as_deref())
        }
        "raiju_predict" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            let prediction_bps_u64 =
                args["prediction_bps"].as_u64().context("prediction_bps required")?;
            let prediction_bps = u16::try_from(prediction_bps_u64)
                .context("prediction_bps must fit in u16 (0-65535)")?;
            client.predict(market_id, agent_id, prediction_bps)
        }
        "raiju_reveal" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            let nostr_secret = raiju::nostr::load_secret_key(None, false)?;
            client.reveal(market_id, agent_id, nostr_secret.as_deref())
        }
        "raiju_trade" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            let direction = args["direction"].as_str().context("direction required")?;
            let shares = args["shares"].as_i64().context("shares required")?;
            client.trade(market_id, agent_id, direction, shares)
        }
        "raiju_leaderboard" => {
            let limit = args["limit"].as_u64();
            let period = args.get("period").and_then(|v| v.as_str());
            client.leaderboard(limit, period)
        }
        "raiju_achievements" => {
            let aid = args.get("agent_id").and_then(|v| v.as_str()).unwrap_or(agent_id);
            client.agent_achievements(aid)
        }
        "raiju_my_status" => client.agent_status(agent_id),
        "raiju_my_positions" => client.positions(agent_id),
        "raiju_consensus" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.consensus(market_id)
        }
        "raiju_get_amm" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.amm_state(market_id)
        }
        "raiju_price_history" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.price_history(market_id)
        }
        "raiju_market_deposits" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.market_deposits(market_id)
        }
        "raiju_market_predictions" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.market_predictions(market_id)
        }
        "raiju_market_stats" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.market_stats(market_id)
        }
        "raiju_market_payouts" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            client.market_payouts(market_id)
        }
        "raiju_trade_history" => {
            let market_id = args.get("market_id").and_then(|v| v.as_str());
            client.trade_history(Some(agent_id), market_id)
        }
        "raiju_actions" => client.agent_actions(agent_id),
        "raiju_claim_payout" => {
            let payout_id = args["payout_id"].as_str().context("payout_id required")?;
            let bolt11 = args["bolt11_invoice"].as_str().context("bolt11_invoice required")?;
            client.claim_payout(payout_id, agent_id, bolt11)
        }
        "raiju_claim_settlement" => {
            let settlement_id = args["settlement_id"].as_str().context("settlement_id required")?;
            let bolt11 = args["bolt11_invoice"].as_str().context("bolt11_invoice required")?;
            client.claim_settlement(settlement_id, agent_id, bolt11)
        }
        "raiju_my_payouts" => client.payouts(agent_id),
        "raiju_my_settlements" => {
            let status = args.get("status").and_then(|v| v.as_str());
            client.settlements(agent_id, status)
        }
        "raiju_list_agents" => {
            let limit = args.get("limit").and_then(serde_json::Value::as_u64);
            let offset = args.get("offset").and_then(serde_json::Value::as_u64);
            client.list_agents(limit, offset)
        }
        "raiju_amm_balance" => {
            let market_id = args["market_id"].as_str().context("market_id required")?;
            let aid = args["agent_id"].as_str().unwrap_or(agent_id);
            client.amm_balance(market_id, aid)
        }
        "raiju_nostr_bind" => {
            let secret_key_hex = raiju::nostr::load_secret_key(None, true)?
                .context("Set RAIJU_NOSTR_SECRET_KEY in the MCP host environment")?;
            client.nostr_bind(&secret_key_hex)
        }
        "raiju_nostr_unbind" => client.nostr_unbind(),
        _ => anyhow::bail!("Unknown tool: {name}"),
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
            "raiju_wallet_set",
            "Connect an NWC (Nostr Wallet Connect) wallet. The server stores the URI encrypted and uses it to pull deposits and push payouts automatically. Supports Alby Hub, LNbits, Zeus, or any NWC-compatible wallet.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "nwc_uri": { "type": "string", "description": "NWC connection URI (nostr+walletconnect://...)" }
                },
                "required": ["nwc_uri"]
            }),
        ),
        tool_def(
            "raiju_wallet_remove",
            "Disconnect the NWC wallet. Reverts to manual BOLT11 deposits and claims.",
            serde_json::json!({"type": "object", "properties": {}}),
        ),
        tool_def(
            "raiju_wallet_status",
            "Check if an NWC wallet is connected and when it was last verified.",
            serde_json::json!({"type": "object", "properties": {}}),
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
        market_tool(
            "raiju_market_payouts",
            "List all BWM payouts for a resolved market with agent IDs, quality scores, and payout amounts",
        ),
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
            "Get your payout history across all resolved markets. BWM payouts use formula: payout = deposit x (10000 + Q - avg_Q) / 10000, where Q is your quality score (10000 - Brier x 10000) and avg_Q is the pool average. Above-average scores profit; below-average lose.",
            serde_json::json!({
                "type": "object", "properties": {}
            }),
        ),
        tool_def(
            "raiju_actions",
            "Get a prioritized list of what you should do right now. Returns reveals due, claimable payouts/settlements, and open markets you haven't entered. Each action has type, priority (urgent/normal/info), deadline, and amount.",
            serde_json::json!({"type": "object", "properties": {}}),
        ),
        tool_def(
            "raiju_claim_payout",
            "Claim a pending BWM payout. With NWC connected, payouts are auto-pushed; use this only if auto-push failed. Provide the payout_id and a BOLT11 invoice for the exact payout amount.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "payout_id": { "type": "string", "description": "Payout ID (from raiju_my_payouts)" },
                    "bolt11_invoice": { "type": "string", "description": "BOLT11 Lightning invoice for exact payout amount" }
                },
                "required": ["payout_id", "bolt11_invoice"]
            }),
        ),
        tool_def(
            "raiju_claim_settlement",
            "Claim a pending AMM settlement. With NWC connected, settlements are auto-pushed; use this only if auto-push failed. Provide the settlement_id and a BOLT11 invoice for total_claimable_sats.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "settlement_id": { "type": "string", "description": "Settlement ID (from raiju_my_settlements)" },
                    "bolt11_invoice": { "type": "string", "description": "BOLT11 Lightning invoice for total claimable amount" }
                },
                "required": ["settlement_id", "bolt11_invoice"]
            }),
        ),
        tool_def(
            "raiju_my_settlements",
            "List your AMM settlements. Use this to discover settlement IDs for pending claims. Shows settlement_sats (token_denomination_sats per winning token, default 10,000), balance_refund_sats (unused AMM balance), and total_claimable_sats. Filter by status to find settlements that need claiming.",
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
#[allow(unsafe_code)]
mod tests {
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

    // ── Commitment hash tests (now via raiju::commitment) ──────────────

    #[test]
    fn test_audit_h10_commitment_hash_test_vectors() {
        assert_eq!(
            raiju::commitment::compute_hash(7200, &[0xAA; 32]),
            "e37bd54454dd1c34d39175ad705ba757dcf1ff7bb0d88ac44b2343d976b214e2"
        );
        assert_eq!(
            raiju::commitment::compute_hash(0, &[0x00; 32]),
            "b947e135b598a4c88194bd667d7c7272c62d6dc0330bfcd539b260c5cffb6c49"
        );
        assert_eq!(
            raiju::commitment::compute_hash(10000, &[0xFF; 32]),
            "f481e72a1b60675d121415e88ab1fc410353f578bc705c991532fec685b470a8"
        );
        let nonce_vec =
            hex::decode("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
                .unwrap();
        let nonce: [u8; 32] = nonce_vec.try_into().unwrap();
        assert_eq!(
            raiju::commitment::compute_hash(5000, &nonce),
            "3a40b116b4687e514e05c98e6c139385934227728ca2dc1c26ad0fe4916e0a44"
        );
    }

    #[test]
    fn test_audit_h15_tool_count() {
        let tools = super::tool_definitions();
        assert!(tools.len() >= 20, "expected at least 20 tools, got {}", tools.len());
    }

    // ── Tool definitions: completeness and structure ────────────────────

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
            "raiju_market_payouts",
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

    #[test]
    fn tool_definitions_no_duplicate_names() {
        let tools = super::tool_definitions();
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name), "duplicate tool name: {name}");
        }
    }

    #[test]
    fn tool_definitions_all_have_required_fields() {
        let tools = super::tool_definitions();
        for (i, tool) in tools.iter().enumerate() {
            assert!(tool["name"].is_string(), "tool[{i}] missing name");
            assert!(tool["description"].is_string(), "tool[{i}] missing description");
            assert!(tool["inputSchema"].is_object(), "tool[{i}] missing inputSchema");
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "tool[{i}] inputSchema type must be 'object'"
            );
        }
    }

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

    #[test]
    fn tool_definitions_no_prohibited_language() {
        let tools = super::tool_definitions();
        let prohibited = ["bet", "wager", "gambling"];
        for tool in &tools {
            let name = tool["name"].as_str().unwrap_or("unknown");
            let desc = tool["description"].as_str().unwrap_or("").to_lowercase();
            for word in &prohibited {
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

    // ── call_tool: unknown tool and argument validation ─────────────────

    #[test]
    fn call_tool_unknown_returns_error() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result =
            super::call_tool(&client, "agent-1", "nonexistent_tool", &serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown tool"), "error should mention 'Unknown tool', got: {err}");
    }

    #[test]
    fn call_tool_empty_name_returns_error() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result = super::call_tool(&client, "agent-1", "", &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn call_tool_deposit_missing_market_id() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"amount_sats": 10000});
        let result = super::call_tool(&client, "agent-1", "raiju_deposit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("market_id"), "should mention market_id, got: {err}");
    }

    #[test]
    fn call_tool_deposit_missing_amount_sats() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id"});
        let result = super::call_tool(&client, "agent-1", "raiju_deposit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("amount_sats"), "should mention amount_sats, got: {err}");
    }

    #[test]
    fn call_tool_commit_missing_market_id() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"prediction_bps": 5000});
        let result = super::call_tool(&client, "agent-1", "raiju_commit", &args);
        assert!(result.is_err());
    }

    #[test]
    fn call_tool_commit_missing_prediction_bps() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id"});
        let result = super::call_tool(&client, "agent-1", "raiju_commit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("prediction_bps"), "should mention prediction_bps, got: {err}");
    }

    #[test]
    fn call_tool_commit_prediction_bps_out_of_range() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": 10001});
        let result = super::call_tool(&client, "agent-1", "raiju_commit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("0-10000"), "should mention valid range, got: {err}");
    }

    #[test]
    fn call_tool_commit_prediction_bps_negative() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": -1});
        let result = super::call_tool(&client, "agent-1", "raiju_commit", &args);
        assert!(result.is_err());
    }

    #[test]
    fn call_tool_commit_prediction_bps_wrong_type() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": "five thousand"});
        let result = super::call_tool(&client, "agent-1", "raiju_commit", &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_commit_retry_same_prediction_preserves_nonce() {
        with_temp_home(|| {
            let agent_id = "11111111-1111-1111-1111-111111111111";
            let client = raiju::client::RaijuClient::new("http://127.0.0.1:1", None);
            let market_id = "550e8400-e29b-41d4-a716-446655440099";

            let first = serde_json::json!({"market_id": market_id, "prediction_bps": 5000});
            let retry = serde_json::json!({"market_id": market_id, "prediction_bps": 5000});

            let _result1 = super::call_tool(&client, agent_id, "raiju_commit", &first);
            let stored1 = raiju::nonce::load(agent_id, market_id).unwrap();

            let _result2 = super::call_tool(&client, agent_id, "raiju_commit", &retry);
            let stored2 = raiju::nonce::load(agent_id, market_id).unwrap();

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
            let client = raiju::client::RaijuClient::new("http://127.0.0.1:1", None);
            let market_id = "550e8400-e29b-41d4-a716-446655440099";

            let first = serde_json::json!({"market_id": market_id, "prediction_bps": 5000});
            let updated = serde_json::json!({"market_id": market_id, "prediction_bps": 7000});

            let _result1 = super::call_tool(&client, agent_id, "raiju_commit", &first);
            let stored1 = raiju::nonce::load(agent_id, market_id).unwrap();

            let _result2 = super::call_tool(&client, agent_id, "raiju_commit", &updated);
            let stored2 = raiju::nonce::load(agent_id, market_id).unwrap();

            assert_eq!(stored2.prediction_bps, 7000, "updated prediction must be stored");
            assert_ne!(
                stored2.nonce, stored1.nonce,
                "different prediction must generate new nonce"
            );
        });
    }

    #[test]
    fn call_tool_commit_prediction_bps_overflow_u16() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id", "prediction_bps": 70000});
        let result = super::call_tool(&client, "agent-1", "raiju_commit", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("u16"), "should mention u16 constraint, got: {err}");
    }

    #[test]
    fn call_tool_trade_missing_direction() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id", "shares": 100});
        let result = super::call_tool(&client, "agent-1", "raiju_trade", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("direction"), "should mention direction, got: {err}");
    }

    #[test]
    fn call_tool_trade_missing_shares() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let args = serde_json::json!({"market_id": "some-id", "direction": "buy_yes"});
        let result = super::call_tool(&client, "agent-1", "raiju_trade", &args);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("shares"), "should mention shares, got: {err}");
    }

    #[test]
    fn call_tool_market_detail_missing_market_id() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result =
            super::call_tool(&client, "agent-1", "raiju_market_detail", &serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("market_id"), "should mention market_id, got: {err}");
    }

    #[test]
    fn call_tool_reveal_missing_market_id() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result = super::call_tool(&client, "agent-1", "raiju_reveal", &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn call_tool_nostr_bind_missing_secret_key() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result = with_nostr_env(None, || {
            super::call_tool(&client, "agent-1", "raiju_nostr_bind", &serde_json::json!({}))
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("RAIJU_NOSTR_SECRET_KEY"),
            "should mention host env secret, got: {err}"
        );
    }

    #[test]
    fn call_tool_nostr_bind_invalid_hex() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result = with_nostr_env(Some("not_hex_at_all!"), || {
            super::call_tool(&client, "agent-1", "raiju_nostr_bind", &serde_json::json!({}))
        });
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("hex"), "should mention hex, got: {err}");
    }

    #[test]
    fn call_tool_nostr_bind_wrong_key_length() {
        let client = raiju::client::RaijuClient::new("http://localhost:9999", None);
        let result = with_nostr_env(Some("aabbccddaabbccddaabbccddaabbccdd"), || {
            super::call_tool(&client, "agent-1", "raiju_nostr_bind", &serde_json::json!({}))
        });
        assert!(result.is_err());
    }

    // ── Nostr event tests (now via raiju::nostr) ───────────────────────

    #[test]
    fn build_prediction_event_valid_key_produces_event() {
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let sk_hex = hex::encode(sk.secret_bytes());

        let tags = serde_json::json!([
            ["d", "raiju:commit:market-1"],
            ["market_id", "market-1"],
            ["commitment_hash", "abc123"]
        ]);

        let event = raiju::nostr::build_event(&sk_hex, 30150, tags).unwrap();

        assert!(event["id"].is_string());
        assert!(event["pubkey"].is_string());
        assert!(event["created_at"].is_number());
        assert_eq!(event["kind"], 30150);
        assert!(event["tags"].is_array());
        assert_eq!(event["content"], "");
        assert!(event["sig"].is_string());
        assert_eq!(event["id"].as_str().unwrap().len(), 64);
        assert_eq!(event["pubkey"].as_str().unwrap().len(), 64);
        assert_eq!(event["sig"].as_str().unwrap().len(), 128);
    }

    #[test]
    fn build_prediction_event_invalid_hex() {
        let result = raiju::nostr::build_event("not_hex", 30150, serde_json::json!([]));
        assert!(result.is_err());
    }

    #[test]
    fn build_prediction_event_wrong_key_length() {
        let result = raiju::nostr::build_event(
            "aabbccddaabbccddaabbccddaabbccdd",
            30150,
            serde_json::json!([]),
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_prediction_event_derives_correct_pubkey() {
        let secp = secp256k1::Secp256k1::new();
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let (xonly, _) = sk.public_key(&secp).x_only_public_key();
        let expected_pubkey = hex::encode(xonly.serialize());

        let sk_hex = hex::encode(sk.secret_bytes());
        let event =
            raiju::nostr::build_event(&sk_hex, 30150, serde_json::json!([["test", "value"]]))
                .unwrap();

        assert_eq!(event["pubkey"].as_str().unwrap(), expected_pubkey);
    }

    #[test]
    fn build_prediction_event_signature_verifies() {
        let sk = secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng());
        let sk_hex = hex::encode(sk.secret_bytes());

        let event =
            raiju::nostr::build_event(&sk_hex, 30150, serde_json::json!([["d", "verify-sig"]]))
                .unwrap();

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

    // ── Market tool schema ─────────────────────────────────────────────

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
            "raiju_market_payouts",
        ];
        for name in &market_tools {
            let tool = tools.iter().find(|t| t["name"] == *name).unwrap();
            let required = tool["inputSchema"]["required"].as_array().unwrap();
            assert_eq!(required.len(), 1);
            assert_eq!(required[0], "market_id", "tool {name} should require market_id");
        }
    }

    #[test]
    fn client_strips_trailing_slash_from_base_url() {
        let client = raiju::client::RaijuClient::new("http://localhost:3001/", None);
        assert_eq!(client.base_url(), "http://localhost:3001");
    }
}
