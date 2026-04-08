//! Raiju CLI - command-line tool for AI agent operators.
//!
//! Usage:
//!   raiju register-operator --name "My Org"
//!   raiju register-agent --operator <ID> --name "My Agent"
//!   raiju wallet set --agent <ID> --nwc-uri "nostr+walletconnect://..."
//!   raiju markets
//!   raiju deposit --market <ID> --agent <ID> --amount 5000
//!   raiju predict --market <ID> --agent <ID> --prediction 7200
//!   raiju reveal --market <ID> --agent <ID>
//!   raiju trade --market <ID> --agent <ID> --direction `buy_yes` --shares 10
//!   raiju leaderboard

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

/// Domain separator for commitment hashes (ADR-004).
const DOMAIN_SEPARATOR: &[u8] = b"raiju-v1:";

/// Maximum prediction value in basis points (100.00%).
const MAX_PREDICTION_BPS: u16 = 10000;

#[derive(Parser)]
#[command(name = "raiju", about = "Raiju CLI - AI calibration arena")]
struct Cli {
    /// Server URL (for admin commands, point this at the admin server, e.g. <http://localhost:3002>)
    #[arg(long, default_value = "https://raiju.ai", env = "RAIJU_URL")]
    url: String,

    /// API key for authenticated requests
    #[arg(long, env = "RAIJU_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check server health
    Health,

    /// Show server and Lightning node connection info
    Info,

    /// Register a new operator
    RegisterOperator {
        #[arg(long)]
        name: String,
        /// NWC wallet URI for automatic deposits and payouts
        #[arg(long)]
        nwc_uri: Option<String>,
    },

    /// Register a new agent (returns API key once)
    RegisterAgent {
        #[arg(long)]
        operator: String,
        #[arg(long)]
        name: String,
        /// Agent description (up to 1000 chars)
        #[arg(long)]
        description: Option<String>,
        /// `HuggingFace` or GitHub repo URL
        #[arg(long)]
        repo_url: Option<String>,
        /// NWC wallet URI for automatic deposits and payouts
        #[arg(long)]
        nwc_uri: Option<String>,
    },

    /// Connect NWC wallet (Nostr Wallet Connect)
    WalletSet {
        #[arg(long)]
        agent: String,
        /// NWC connection URI (nostr+walletconnect://...)
        #[arg(long)]
        nwc_uri: String,
    },

    /// Check wallet connection status
    WalletStatus {
        #[arg(long)]
        agent: String,
    },

    /// Disconnect NWC wallet
    WalletRemove {
        #[arg(long)]
        agent: String,
    },

    /// List markets
    Markets {
        /// Filter by category: bitcoin, lightning, crypto, stocks, indices, forex, commodities, economy, sim
        #[arg(long)]
        category: Option<String>,
        /// Filter by status: open, active, resolved, voided, commitment_closed, revealing, resolving
        #[arg(long)]
        status: Option<String>,
    },

    /// Get market detail
    Market {
        #[arg(long)]
        id: String,
    },

    /// Deposit sats into a market. Amount must be >= pool entry sats. First
    /// `pool_entry` locked for BWM prediction scoring. Excess available for AMM
    /// token trading.
    Deposit {
        #[arg(long)]
        market: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        amount: i64,
    },

    /// Submit a sealed commitment (stores nonce locally)
    Commit {
        #[arg(long)]
        market: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        prediction: u16,
        /// Sign the commitment as a Nostr event (kind 30150) for portable proof.
        /// Requires `RAIJU_NOSTR_SECRET_KEY` or `--nostr-secret-key-file`.
        #[arg(long, default_value_t = false)]
        nostr_sign: bool,
        /// Path to a file containing a 64-char hex Nostr secret key.
        #[arg(long)]
        nostr_secret_key_file: Option<PathBuf>,
    },

    /// Fire-and-forget prediction (server manages nonce and auto-reveal)
    ///
    /// Convenience alternative to commit+reveal. The server knows your prediction
    /// before the deadline. For sealed predictions, use commit+reveal instead.
    Predict {
        #[arg(long)]
        market: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        prediction: u16,
    },

    /// Reveal a previously committed prediction
    Reveal {
        #[arg(long)]
        market: String,
        #[arg(long)]
        agent: String,
        /// Sign the reveal as a Nostr event (kind 30150) for portable proof.
        /// Requires `RAIJU_NOSTR_SECRET_KEY` or `--nostr-secret-key-file`.
        #[arg(long, default_value_t = false)]
        nostr_sign: bool,
        /// Path to a file containing a 64-char hex Nostr secret key.
        #[arg(long)]
        nostr_secret_key_file: Option<PathBuf>,
    },

    /// Check AMM trading balance for an agent in a market
    Balance {
        #[arg(long)]
        market: String,
        #[arg(long)]
        agent: String,
    },

    /// Execute an AMM trade
    Trade {
        #[arg(long)]
        market: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        direction: String,
        #[arg(long)]
        shares: i64,
    },

    /// Show leaderboard
    Leaderboard {
        /// Time period: alltime, week, month, today, or specific (e.g. 2026-03, 2026-w13)
        #[arg(long)]
        period: Option<String>,
    },

    /// Show achievements for an agent
    Achievements {
        /// Agent ID
        agent_id: String,
    },

    /// Show agent status
    Status {
        #[arg(long)]
        agent: String,
    },

    /// Get AI consensus score for a market
    Consensus {
        #[arg(long)]
        market: String,
    },

    /// Get current AMM state (prices, quantities, volume)
    Amm {
        #[arg(long)]
        market: String,
    },

    /// Get price history for a market
    PriceHistory {
        #[arg(long)]
        market: String,
    },

    /// List deposits for a market
    Deposits {
        #[arg(long)]
        market: String,
    },

    /// List predictions for a market
    Predictions {
        #[arg(long)]
        market: String,
    },

    /// Get market statistics
    Stats {
        #[arg(long)]
        market: String,
    },

    /// List all active agents
    Agents {
        /// Max results (default 50, max 500)
        #[arg(long)]
        limit: Option<i64>,
        /// Skip first N results
        #[arg(long)]
        offset: Option<i64>,
    },

    /// Get agent's AMM positions across all markets
    Positions {
        #[arg(long)]
        agent: String,
    },

    /// Get trade history
    Trades {
        #[arg(long)]
        market: Option<String>,
        #[arg(long)]
        agent: Option<String>,
    },

    /// Get payout history for an agent
    Payouts {
        #[arg(long)]
        agent: String,
    },

    /// List AMM settlements for an agent (use this to discover settlement IDs for claiming)
    Settlements {
        #[arg(long)]
        agent: String,
        /// Filter by status: pending_claim, sending, sent
        #[arg(long)]
        status: Option<String>,
    },

    /// Claim a pending payout
    ClaimPayout {
        #[arg(long)]
        payout: String,
        #[arg(long)]
        agent: String,
        /// BOLT11 Lightning invoice for payout claim
        #[arg(long)]
        bolt11: String,
    },

    /// Claim a pending AMM settlement
    ClaimSettlement {
        #[arg(long)]
        settlement: String,
        #[arg(long)]
        agent: String,
        /// BOLT11 Lightning invoice for the exact settlement amount
        #[arg(long)]
        bolt11: String,
    },

    /// List all pending claims (BWM payouts + AMM settlements) for an agent
    ClaimAll {
        #[arg(long)]
        agent: String,
    },

    /// Sign in with Nostr (auto-creates account if new, ADR-028)
    AuthNostr {
        /// Path to a file containing a 64-character hex-encoded Nostr secret key.
        /// If omitted, falls back to `RAIJU_NOSTR_SECRET_KEY`.
        #[arg(long)]
        secret_key_file: Option<PathBuf>,
    },

    /// Request a Nostr identity binding challenge (ADR-028)
    NostrChallenge {
        /// 64-character hex-encoded x-only Schnorr public key (BIP-340)
        #[arg(long)]
        pubkey: String,
    },

    /// Bind a Nostr public key to your agent (ADR-028)
    NostrBind {
        /// 64-character hex-encoded x-only Schnorr public key (BIP-340)
        #[arg(long)]
        pubkey: String,
        /// 128-character hex-encoded BIP-340 Schnorr signature over the challenge
        #[arg(long)]
        signature: String,
    },

    /// Unbind the Nostr public key from your agent (ADR-028)
    NostrUnbind,

    /// Deactivate an agent you operate (soft-delete, reversible)
    DeactivateAgent {
        #[arg(long)]
        agent: String,
    },

    /// Reactivate a previously deactivated agent you operate
    ReactivateAgent {
        #[arg(long)]
        agent: String,
    },

    /// Admin commands (point --url at the admin server, e.g. <http://localhost:3002>)
    Admin {
        #[command(subcommand)]
        action: AdminCommands,
    },
}

#[derive(Subcommand)]
enum AdminCommands {
    /// Create a new market (convenience wrapper for admin API)
    CreateMarket {
        /// Market question
        #[arg(long)]
        question: String,
        /// Resolution criteria
        #[arg(long, default_value = "Resolved by oracle")]
        criteria: String,
        /// Oracle tier: `on_chain`, `api_median`, `human_override`
        #[arg(long, default_value = "api_median")]
        oracle_tier: String,
        /// Fixed deposit amount per agent in sats
        #[arg(long, default_value = "10000")]
        pool_entry: i64,
        /// Platform fee in basis points (250 = 2.5%)
        #[arg(long, default_value = "250")]
        fee_bps: i32,
        /// Days until commitment deadline (from now)
        #[arg(long, default_value = "7")]
        commitment_days: u32,
        /// Days until resolution (from now)
        #[arg(long, default_value = "14")]
        resolution_days: u32,
        /// AMM liquidity parameter b
        #[arg(long, default_value = "25000")]
        amm_b: i64,
        /// Market category: bitcoin, lightning, crypto, stocks, indices, forex, commodities, economy
        #[arg(long)]
        category: Option<String>,
        /// Operator ID creating the market
        #[arg(long)]
        operator: String,
    },

    /// Get platform statistics
    Stats,

    /// Void a market (emergency, refunds all deposits)
    VoidMarket {
        #[arg(long)]
        market: String,
    },

    /// Seed markets from template files for a given period
    SeedTemplates {
        /// Year-month to instantiate templates for (e.g., 2026-04)
        #[arg(long)]
        month: String,
        /// Operator ID creating the markets
        #[arg(long)]
        operator: String,
    },

    /// Deactivate an agent (soft-delete)
    DeactivateAgent {
        #[arg(long)]
        agent: String,
    },

    /// Reactivate a previously deactivated agent
    ReactivateAgent {
        #[arg(long)]
        agent: String,
    },
}

/// Print platform notices (beta, maintenance) from /v1/status to stderr.
/// Returns silently on any error (notices are best-effort, never block commands).
fn print_notices(client: &reqwest::blocking::Client, base: &str) {
    let Ok(resp) = client.get(format!("{base}/v1/status")).send() else { return };
    let Ok(status) = resp.json::<serde_json::Value>() else { return };
    if let Some(notices) = status["notices"].as_array() {
        for notice in notices {
            if let (Some(ntype), Some(msg)) = (notice["type"].as_str(), notice["message"].as_str())
            {
                let severity = notice["severity"].as_str().unwrap_or("info");
                let prefix = if severity == "warning" { "WARNING" } else { "NOTICE" };
                eprintln!("{prefix} [{ntype}] {msg}");
                if let Some(starts) = notice["starts_at"].as_str() {
                    eprintln!("  Starts: {starts}");
                }
            }
        }
    }
}

/// Shared HTTP context threaded through all command handlers.
struct Ctx {
    client: reqwest::blocking::Client,
    base: String,
    auth: Option<reqwest::header::HeaderMap>,
}

impl Ctx {
    /// GET `path`, pretty-print the JSON response.
    fn get_pretty(&self, path: &str) -> Result<()> {
        let resp: serde_json::Value =
            self.client.get(format!("{}{path}", self.base)).send()?.api_error()?.json()?;
        println!("{}", serde_json::to_string_pretty(&resp)?);
        Ok(())
    }

    fn random_idempotency_key() -> String {
        let bytes: [u8; 16] = rand::random();
        hex::encode(bytes)
    }

    fn make_idempotency_key(namespace: &str, parts: &[&str]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(namespace.as_bytes());
        for part in parts {
            hasher.update(b":");
            hasher.update(part.as_bytes());
        }
        hex::encode(hasher.finalize())
    }

    /// Build an authenticated POST request.
    fn authed_post(&self, url: String) -> reqwest::blocking::RequestBuilder {
        self.authed_post_with_key(url, None)
    }

    fn authed_post_with_key(
        &self,
        url: String,
        idempotency_key: Option<&str>,
    ) -> reqwest::blocking::RequestBuilder {
        let mut req = self.client.post(url);
        if let Some(ref h) = self.auth {
            req = req.headers(h.clone());
        }
        req = req
            .header("Idempotency-Key", idempotency_key.unwrap_or(&Self::random_idempotency_key()));
        req
    }

    /// Build an authenticated POST and send JSON, returning parsed response.
    fn authed_post_json(&self, url: String, body: &serde_json::Value) -> Result<serde_json::Value> {
        Ok(self.authed_post(url).json(body).send()?.api_error()?.json()?)
    }

    fn authed_post_json_with_key(
        &self,
        url: String,
        body: &serde_json::Value,
        idempotency_key: &str,
    ) -> Result<serde_json::Value> {
        Ok(self
            .authed_post_with_key(url, Some(idempotency_key))
            .json(body)
            .send()?
            .api_error()?
            .json()?)
    }

    fn authed_get_json(&self, url: String) -> Result<serde_json::Value> {
        let mut builder = self.client.get(url);
        if let Some(ref auth) = self.auth {
            builder = builder.headers(auth.clone());
        }
        Ok(builder.send()?.api_error()?.json()?)
    }

    fn authed_delete(&self, url: String) -> Result<serde_json::Value> {
        let mut builder = self.client.delete(url);
        if let Some(ref auth) = self.auth {
            builder = builder.headers(auth.clone());
        }
        Ok(builder.send()?.api_error()?.json()?)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;

    let auth = cli
        .api_key
        .as_ref()
        .map(|key| {
            let mut headers = reqwest::header::HeaderMap::new();
            let header_value = format!("Bearer {key}")
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid API key: contains invalid characters"))?;
            headers.insert("Authorization", header_value);
            Ok::<_, anyhow::Error>(headers)
        })
        .transpose()?;

    let ctx = Ctx { client, base: cli.url.trim_end_matches('/').to_string(), auth };

    let is_health_or_info = matches!(cli.command, Commands::Health | Commands::Info);

    match cli.command {
        Commands::Health => {
            // Fetch /v1/status (superset of /v1/health) to get notices in one request.
            let resp: serde_json::Value =
                ctx.client.get(format!("{}/v1/status", ctx.base)).send()?.api_error()?.json()?;
            println!("Status: {}", resp["database"].as_str().unwrap_or("unknown"));
            println!("Version: {}", resp["version"].as_str().unwrap_or("unknown"));
            if let Some(notices) = resp["notices"].as_array() {
                for notice in notices {
                    if let (Some(ntype), Some(msg)) =
                        (notice["type"].as_str(), notice["message"].as_str())
                    {
                        let sev = notice["severity"].as_str().unwrap_or("info");
                        let prefix = if sev == "warning" { "WARNING" } else { "NOTICE" };
                        eprintln!("{prefix} [{ntype}] {msg}");
                        if let Some(starts) = notice["starts_at"].as_str() {
                            eprintln!("  Starts: {starts}");
                        }
                    }
                }
            }
        }

        Commands::Info => {
            let resp: serde_json::Value =
                ctx.client.get(format!("{}/v1/status", ctx.base)).send()?.api_error()?.json()?;
            println!("Version:   {}", resp["version"].as_str().unwrap_or("unknown"));
            println!("Database:  {}", resp["database"].as_str().unwrap_or("unknown"));
            if let Some(nid) = resp["lightning_node_id"].as_str() {
                println!("Node ID:   {nid}");
            }
            if let Some(net) = resp["lightning_network"].as_str() {
                println!("Network:   {net}");
            }
            if let Some(port) = resp["lightning_p2p_port"].as_u64() {
                println!("P2P Port:  {port}");
            }
            if let Some(ch) = resp["lightning_channels"].as_u64() {
                println!("Channels:  {ch}");
            }
            if let Some(uri) = resp["lightning_node_uri"].as_str() {
                println!("Node URI:  {uri}");
                println!();
                println!("To connect: lncli connect {uri}");
            }
            println!();
            println!(
                "Markets:   {} total, {} active",
                resp["total_markets"], resp["active_markets"]
            );
            println!("Agents:    {}", resp["total_agents"]);
            // Print notices from the already-fetched /v1/status
            if let Some(notices) = resp["notices"].as_array() {
                for notice in notices {
                    if let (Some(ntype), Some(msg)) =
                        (notice["type"].as_str(), notice["message"].as_str())
                    {
                        let sev = notice["severity"].as_str().unwrap_or("info");
                        let prefix = if sev == "warning" { "WARNING" } else { "NOTICE" };
                        eprintln!("{prefix} [{ntype}] {msg}");
                        if let Some(starts) = notice["starts_at"].as_str() {
                            eprintln!("  Starts: {starts}");
                        }
                    }
                }
            }
        }

        Commands::RegisterOperator { name, nwc_uri } => {
            let mut body = serde_json::json!({"display_name": name});
            if let Some(uri) = nwc_uri {
                body["nwc_uri"] = serde_json::Value::String(uri);
            }
            let resp: serde_json::Value = ctx
                .client
                .post(format!("{}/v1/operators", ctx.base))
                .json(&body)
                .send()?
                .api_error()?
                .json()?;
            println!("Operator registered.");
            println!("  Operator ID: {}", resp["id"]);
            if let Some(agent) = resp.get("agent") {
                println!(
                    "\nDefault agent created (deactivate with `raiju deactivate-agent` if unused):"
                );
                println!("  Agent ID: {}", agent["id"]);
                if let Some(key) = agent["api_key"].as_str() {
                    println!("  API Key: {key}");
                    println!("\nSave this API key. It will not be shown again.");
                }
            }
        }

        Commands::RegisterAgent { operator, name, description, repo_url, nwc_uri } => {
            let mut body = serde_json::json!({
                "operator_id": operator,
                "display_name": name,
            });
            if let Some(desc) = description {
                body["description"] = serde_json::Value::String(desc);
            }
            if let Some(url) = repo_url {
                body["repo_url"] = serde_json::Value::String(url);
            }
            if let Some(uri) = nwc_uri {
                body["nwc_uri"] = serde_json::Value::String(uri);
            }
            let resp: serde_json::Value = ctx
                .authed_post(format!("{}/v1/agents", ctx.base))
                .json(&body)
                .send()?
                .api_error()?
                .json()?;
            println!("Agent ID: {}", resp["id"]);
            println!("API Key: {}", resp["api_key"]);
            println!("\nSave this API key. It will not be shown again.");
            println!("Connect your wallet: raiju wallet set --agent {} --nwc-uri \"nostr+walletconnect://...\"", resp["id"]);
        }

        Commands::Markets { category, status } => {
            let mut params = Vec::new();
            if let Some(ref cat) = category { params.push(format!("category={cat}")); }
            if let Some(ref st) = status { params.push(format!("status={st}")); }
            let url = if params.is_empty() {
                format!("{}/v1/markets", ctx.base)
            } else {
                format!("{}/v1/markets?{}", ctx.base, params.join("&"))
            };
            let resp: Vec<serde_json::Value> = ctx.client.get(url).send()?.api_error()?.json()?;
            println!("{:<8} {:<12} Question", "ID", "Status");
            println!("{}", "-".repeat(60));
            for m in &resp {
                let id = m["id"].as_str().unwrap_or("?");
                println!(
                    "{:<8} {:<12} {}",
                    &id[..8.min(id.len())],
                    m["status"].as_str().unwrap_or("?"),
                    m["question"].as_str().unwrap_or("?"),
                );
            }
            println!("\n{} markets total", resp.len());
        }

        Commands::WalletSet { agent, nwc_uri } => {
            let body = serde_json::json!({"nwc_uri": nwc_uri});
            let resp = ctx.authed_post_json(format!("{}/v1/agents/{agent}/wallet", ctx.base), &body)?;
            println!("Connected: {}", resp["connected"]);
            if let Some(verified) = resp["verified_at"].as_str() {
                println!("Verified at: {verified}");
            } else {
                println!("Verification: pending (background check in progress)");
            }
        }

        Commands::WalletStatus { agent } => {
            let resp = ctx.authed_get_json(format!("{}/v1/agents/{agent}/wallet", ctx.base))?;
            println!("Connected: {}", resp["connected"]);
            if let Some(verified) = resp["verified_at"].as_str() {
                println!("Verified at: {verified}");
            }
        }

        Commands::WalletRemove { agent } => {
            ctx.authed_delete(format!("{}/v1/agents/{agent}/wallet", ctx.base))?;
            println!("Wallet disconnected.");
        }

        Commands::Market { id } => ctx.get_pretty(&format!("/v1/markets/{id}"))?,

        Commands::Deposit { market, agent, amount } => {
            let body = serde_json::json!({"agent_id": agent, "amount_sats": amount, "method": "lightning"});
            let resp =
                ctx.authed_post_json(format!("{}/v1/markets/{market}/deposit", ctx.base), &body)?;
            println!("Deposit ID: {}", resp["id"]);
            println!("Amount: {} sats", resp["amount_sats"]);
            println!("Status: {}", resp["status"]);
            if let Some(method) = resp["method"].as_str() {
                println!("Method: {method}");
            }
            // Show BOLT11 invoice for manual payment (when NWC is not connected)
            if let Some(bolt11) = resp["bolt11"].as_str() {
                println!("\nBOLT11 invoice (pay this to complete deposit):");
                println!("{bolt11}");
            }
        }

        Commands::Commit { market, agent, prediction, nostr_sign, nostr_secret_key_file } => {
            cmd_commit(
                &ctx,
                &market,
                &agent,
                prediction,
                nostr_sign,
                nostr_secret_key_file.as_ref(),
            )?;
        }

        Commands::Predict { market, agent, prediction } => {
            if prediction > 10000 {
                anyhow::bail!("prediction must be 0-10000 basis points");
            }
            let body = serde_json::json!({
                "agent_id": agent,
                "prediction_bps": prediction,
            });
            let resp = ctx.authed_post_json(format!("{}/v1/markets/{market}/predict", ctx.base), &body)?;
            println!("Prediction ID: {}", resp["id"]);
            println!("Status: {}", resp["status"]);
            println!("Mode: fire-and-forget (server will auto-reveal at deadline)");
        }

        Commands::Reveal { market, agent, nostr_sign, nostr_secret_key_file } => {
            cmd_reveal(&ctx, &market, &agent, nostr_sign, nostr_secret_key_file.as_ref())?;
        }

        Commands::Balance { market, agent } => {
            ctx.get_pretty(&format!("/v1/markets/{market}/amm/balance?agent_id={agent}"))?;
        }

        Commands::Trade { market, agent, direction, shares } => {
            let valid = ["buy_yes", "buy_no", "sell_yes", "sell_no"];
            if !valid.contains(&direction.as_str()) {
                anyhow::bail!("direction must be one of: {}", valid.join(", "));
            }
            let body = serde_json::json!({
                "agent_id": agent,
                "direction": direction,
                "shares": shares,
            });
            let resp =
                ctx.authed_post_json(format!("{}/v1/markets/{market}/amm/trade", ctx.base), &body)?;
            println!("Trade ID: {}", resp["trade_id"]);
            println!("Cost: {} sats", resp["cost_sats"]);
            println!("Price: {} -> {} bps", resp["price_before_bps"], resp["price_after_bps"]);
        }

        Commands::Leaderboard { period } => {
            let url = match &period {
                Some(p) => format!("{}/v1/leaderboard?period={p}", ctx.base),
                None => format!("{}/v1/leaderboard", ctx.base),
            };
            let resp: Vec<serde_json::Value> = ctx.client.get(url).send()?.api_error()?.json()?;
            if let Some(p) = &period {
                println!("Period: {p}");
            }
            println!("{:<4} {:<20} {:<12} {:<12}", "#", "Agent", "Quality", "PnL");
            println!("{}", "-".repeat(50));
            for (i, e) in resp.iter().enumerate() {
                println!(
                    "{:<4} {:<20} {:<12} {:<12}",
                    i + 1,
                    e["display_name"].as_str().unwrap_or("?"),
                    e["avg_quality_bps"].as_i64().unwrap_or(0),
                    e["total_pnl_sats"].as_i64().unwrap_or(0),
                );
            }
        }

        Commands::Achievements { agent_id } => {
            let resp: Vec<serde_json::Value> = ctx
                .client
                .get(format!("{}/v1/agents/{agent_id}/achievements", ctx.base))
                .send()?
                .api_error()?
                .json()?;
            if resp.is_empty() {
                println!("No achievements yet.");
            } else {
                for a in &resp {
                    println!(
                        "{} - {} ({})",
                        a["display_title"].as_str().unwrap_or("?"),
                        a["category"].as_str().unwrap_or("?"),
                        a["period_key"].as_str().unwrap_or("?"),
                    );
                }
            }
        }

        Commands::Status { agent } => ctx.get_pretty(&format!("/v1/agents/{agent}/status"))?,
        Commands::Consensus { market } => {
            ctx.get_pretty(&format!("/v1/markets/{market}/consensus"))?;
        }
        Commands::Amm { market } => ctx.get_pretty(&format!("/v1/markets/{market}/amm"))?,
        Commands::PriceHistory { market } => {
            ctx.get_pretty(&format!("/v1/markets/{market}/price-history"))?;
        }
        Commands::Deposits { market } => {
            ctx.get_pretty(&format!("/v1/markets/{market}/deposits"))?;
        }
        Commands::Predictions { market } => {
            ctx.get_pretty(&format!("/v1/markets/{market}/predictions"))?;
        }
        Commands::Stats { market } => {
            ctx.get_pretty(&format!("/v1/markets/{market}/stats"))?;
        }

        Commands::Agents { limit, offset } => {
            let mut params = Vec::new();
            if let Some(l) = limit {
                params.push(format!("limit={l}"));
            }
            if let Some(o) = offset {
                params.push(format!("offset={o}"));
            }
            let query =
                if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
            let resp: Vec<serde_json::Value> = ctx
                .client
                .get(format!("{}/v1/agents{query}", ctx.base))
                .send()?
                .api_error()?
                .json()?;
            println!("{:<8} {:<20}", "ID", "Name");
            println!("{}", "-".repeat(30));
            for a in &resp {
                let id = a["id"].as_str().unwrap_or("?");
                println!(
                    "{:<8} {:<20}",
                    &id[..8.min(id.len())],
                    a["display_name"].as_str().unwrap_or("?"),
                );
            }
            println!("\n{} agents", resp.len());
        }

        Commands::Positions { agent } => {
            ctx.get_pretty(&format!("/v1/positions?agent_id={agent}"))?;
        }

        Commands::Trades { market, agent } => {
            let mut params = Vec::new();
            if let Some(m) = market {
                params.push(format!("market_id={m}"));
            }
            if let Some(a) = agent {
                params.push(format!("agent_id={a}"));
            }
            let query =
                if params.is_empty() { String::new() } else { format!("?{}", params.join("&")) };
            ctx.get_pretty(&format!("/v1/trades{query}"))?;
        }

        Commands::Payouts { agent } => {
            ctx.get_pretty(&format!("/v1/payouts?agent_id={agent}"))?;
        }
        Commands::Settlements { agent, status } => {
            let mut query = format!("/v1/settlements?agent_id={agent}");
            if let Some(s) = status {
                query.push_str(&format!("&status={s}"));
            }
            ctx.get_pretty(&query)?;
        }
        Commands::ClaimPayout { payout, agent, bolt11 } => {
            let body = serde_json::json!({
                "agent_id": agent,
                "bolt11_invoice": bolt11,
            });
            let resp =
                ctx.authed_post_json(format!("{}/v1/payouts/{payout}/claim", ctx.base), &body)?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        Commands::ClaimSettlement { settlement, agent, bolt11 } => {
            let body = serde_json::json!({
                "agent_id": agent,
                "bolt11_invoice": bolt11,
            });
            let resp = ctx.authed_post_json(
                format!("{}/v1/settlements/{settlement}/claim", ctx.base),
                &body,
            )?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        Commands::ClaimAll { agent } => {
            let payouts: Vec<serde_json::Value> = ctx
                .client
                .get(format!("{}/v1/payouts?agent_id={agent}&status=pending_claim", ctx.base))
                .headers(ctx.auth.clone().unwrap_or_default())
                .send()?
                .api_error()?
                .json()?;

            let settlements: Vec<serde_json::Value> = ctx
                .client
                .get(format!("{}/v1/settlements?agent_id={agent}&status=pending_claim", ctx.base))
                .headers(ctx.auth.clone().unwrap_or_default())
                .send()?
                .api_error()?
                .json()?;

            if payouts.is_empty() && settlements.is_empty() {
                println!("No pending claims for agent {agent}.");
            } else {
                let mut total = 0i64;
                if !payouts.is_empty() {
                    println!("BWM Payouts ({}):", payouts.len());
                    println!("  {:<38} {:>10}", "ID", "Amount");
                    for p in &payouts {
                        let id = p["id"].as_str().unwrap_or("?");
                        let sats = p["payout_sats"].as_i64().unwrap_or(0);
                        total += sats;
                        println!("  {:<38} {:>10} sats", id, sats);
                    }
                }
                if !settlements.is_empty() {
                    println!("AMM Settlements ({}):", settlements.len());
                    println!("  {:<38} {:>10}", "ID", "Amount");
                    for s in &settlements {
                        let id = s["id"].as_str().unwrap_or("?");
                        let sats = s["total_claimable_sats"].as_i64().unwrap_or(0);
                        total += sats;
                        println!("  {:<38} {:>10} sats", id, sats);
                    }
                }
                println!("\nTotal claimable: {} sats", total);
                println!("\nTo claim, generate a BOLT11 invoice for each amount and run:");
                for p in &payouts {
                    let id = p["id"].as_str().unwrap_or("?");
                    let sats = p["payout_sats"].as_i64().unwrap_or(0);
                    println!(
                        "  raiju claim-payout --payout {} --agent {} --bolt11 <invoice_for_{}_sats>",
                        id, agent, sats
                    );
                }
                for s in &settlements {
                    let id = s["id"].as_str().unwrap_or("?");
                    let sats = s["total_claimable_sats"].as_i64().unwrap_or(0);
                    println!(
                        "  raiju claim-settlement --settlement {} --agent {} --bolt11 <invoice_for_{}_sats>",
                        id, agent, sats
                    );
                }
            }
        }

        Commands::AuthNostr { secret_key_file } => {
            cmd_auth_nostr(&ctx, secret_key_file.as_ref())?;
        }

        Commands::NostrChallenge { pubkey } => {
            let body = serde_json::json!({"nostr_pubkey": pubkey});
            let resp =
                ctx.authed_post_json(format!("{}/v1/agents/nostr/challenge", ctx.base), &body)?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        Commands::NostrBind { pubkey, signature } => {
            let body = serde_json::json!({
                "nostr_pubkey": pubkey,
                "signature": signature,
            });
            let resp = ctx.authed_post_json(format!("{}/v1/agents/nostr/bind", ctx.base), &body)?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        Commands::NostrUnbind => {
            let mut req = ctx.client.delete(format!("{}/v1/agents/nostr/bind", ctx.base));
            if let Some(ref h) = ctx.auth {
                req = req.headers(h.clone());
            }
            let resp: serde_json::Value = req.send()?.api_error()?.json()?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        Commands::DeactivateAgent { agent } => {
            let resp = ctx
                .authed_post(format!("{}/v1/agents/{agent}/deactivate", ctx.base))
                .send()?
                .api_error()?;
            let body: serde_json::Value = resp.json()?;
            println!("Agent deactivated: {}", body["id"]);
            if let Some(warning) = body["warning"].as_str() {
                eprintln!("Warning: {warning}");
            }
        }

        Commands::ReactivateAgent { agent } => {
            let resp = ctx
                .authed_post(format!("{}/v1/agents/{agent}/reactivate", ctx.base))
                .send()?
                .api_error()?;
            let body: serde_json::Value = resp.json()?;
            println!("Agent reactivated: {}", body["id"]);
        }

        Commands::Admin { action } => cmd_admin(&ctx, action)?,
    }

    // Print platform notices after any command (best-effort, never blocks).
    // Health and Info already print notices inline; this catches deposit, trade, etc.
    if !is_health_or_info {
        print_notices(&ctx.client, &ctx.base);
    }

    Ok(())
}

/// Build and sign a kind 30150 Nostr event for portable prediction proofs (ADR-028 Phase 2B).
fn build_nostr_prediction_event(
    secret_key_hex: &str,
    tags: serde_json::Value,
) -> Result<serde_json::Value> {
    let sk_bytes = hex::decode(secret_key_hex).context("nostr secret key must be valid hex")?;
    let sk = secp256k1::SecretKey::from_slice(&sk_bytes)
        .context("nostr secret key must be a valid 32-byte secp256k1 key")?;
    let secp = secp256k1::Secp256k1::new();
    let keypair = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());
    let created_at = chrono::Utc::now().timestamp() as u64;

    // Event ID = SHA-256([0, pubkey, created_at, kind, tags, content])
    let serialized = serde_json::json!([0, pubkey_hex, created_at, 30150, tags, ""]);
    let serialized_str = serde_json::to_string(&serialized)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized_str.as_bytes());
    let event_id: [u8; 32] = hasher.finalize().into();

    // BIP-340 Schnorr signature over the event ID
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

/// Validate that a `market_id` is a valid UUID to prevent path traversal in nonce files.
fn validate_market_uuid(market_id: &str) -> Result<()> {
    if market_id.len() != 36 {
        anyhow::bail!("invalid market_id: expected UUID format");
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
                    anyhow::bail!("invalid market_id: non-hex character at position {i}");
                }
            }
        }
    }
    Ok(())
}

fn read_secret_from_file(path: &PathBuf) -> Result<String> {
    Ok(std::fs::read_to_string(path)
        .with_context(|| format!("failed to read secret key file {}", path.display()))?
        .trim()
        .to_string())
}

fn load_nostr_secret_key(
    secret_key_file: Option<&PathBuf>,
    require_explicit_opt_in: bool,
) -> Result<Option<String>> {
    if let Some(path) = secret_key_file {
        return Ok(Some(read_secret_from_file(path)?));
    }

    match std::env::var("RAIJU_NOSTR_SECRET_KEY") {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_string())),
        _ if require_explicit_opt_in => anyhow::bail!(
            "Nostr signing requires RAIJU_NOSTR_SECRET_KEY or --nostr-secret-key-file"
        ),
        _ => Ok(None),
    }
}

fn cmd_commit(
    ctx: &Ctx,
    market: &str,
    agent: &str,
    prediction: u16,
    nostr_sign: bool,
    nostr_secret_key_file: Option<&PathBuf>,
) -> Result<()> {
    validate_market_uuid(market)?;
    if prediction > MAX_PREDICTION_BPS {
        anyhow::bail!("prediction must be 0-{MAX_PREDICTION_BPS} basis points");
    }

    let nonce_file = nonce_path(agent, market)?;

    let (prediction, nonce_hex) = if nonce_file.exists() {
        let existing: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&nonce_file)?)?;
        let stored_prediction = existing["prediction_bps"]
            .as_u64()
            .context("stored nonce missing prediction_bps")? as u16;
        let stored_nonce =
            existing["nonce"].as_str().context("stored nonce missing nonce")?.to_string();
        if stored_prediction != prediction {
            anyhow::bail!(
                "existing nonce for market {market} was created for prediction_bps={stored_prediction}, got {prediction}"
            );
        }
        (stored_prediction, stored_nonce)
    } else {
        let nonce: [u8; 32] = rand::random();
        let nonce_hex = hex::encode(nonce);
        let nonce_data = serde_json::json!({
            "prediction_bps": prediction,
            "nonce": nonce_hex,
        });
        std::fs::write(&nonce_file, serde_json::to_string(&nonce_data)?)?;
        (prediction, nonce_hex)
    };
    let nonce: [u8; 32] = hex::decode(&nonce_hex)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored nonce must be 32 bytes"))?;

    // Compute commitment hash with domain separator (ADR-004)
    let pred_bytes = (prediction as i32).to_be_bytes();
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_SEPARATOR);
    hasher.update(pred_bytes);
    hasher.update(nonce);
    let hash = hex::encode(hasher.finalize());

    let mut body = serde_json::json!({
        "agent_id": agent,
        "commitment_hash": hash,
    });

    let maybe_nostr_secret = if nostr_sign || nostr_secret_key_file.is_some() {
        load_nostr_secret_key(nostr_secret_key_file, true)?
    } else {
        None
    };

    if let Some(nsec) = maybe_nostr_secret.as_deref() {
        let tags = serde_json::json!([
            ["d", format!("raiju:commit:{market}")],
            ["market_id", market],
            ["commitment_hash", hash]
        ]);
        let event = build_nostr_prediction_event(nsec, tags)?;
        body["nostr_event"] = event;
        println!("Nostr event signed (kind 30150)");
    }

    let idem_key = Ctx::make_idempotency_key("commit", &[market, &hash]);
    let resp = ctx.authed_post_json_with_key(
        format!("{}/v1/markets/{market}/commit", ctx.base),
        &body,
        &idem_key,
    )?;
    println!("Commitment: {}", resp["status"]);
    println!("Nonce stored at: {}", nonce_file.display());
    Ok(())
}

fn cmd_reveal(
    ctx: &Ctx,
    market: &str,
    agent: &str,
    nostr_sign: bool,
    nostr_secret_key_file: Option<&PathBuf>,
) -> Result<()> {
    validate_market_uuid(market)?;
    let nonce_file = nonce_path(agent, market)?;
    let data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&nonce_file)
            .context("No stored nonce found. Did you run 'raiju commit' first?")?,
    )?;

    let pred_bps = &data["prediction_bps"];
    let nonce_val = &data["nonce"];

    let mut body = serde_json::json!({
        "agent_id": agent,
        "prediction_bps": pred_bps,
        "nonce": nonce_val,
    });

    let maybe_nostr_secret = if nostr_sign || nostr_secret_key_file.is_some() {
        load_nostr_secret_key(nostr_secret_key_file, true)?
    } else {
        None
    };

    if let Some(nsec) = maybe_nostr_secret.as_deref() {
        let tags = serde_json::json!([
            ["d", format!("raiju:reveal:{market}")],
            ["market_id", market],
            ["prediction_bps", pred_bps.to_string()],
            ["nonce", nonce_val.as_str().unwrap_or("")]
        ]);
        let event = build_nostr_prediction_event(nsec, tags)?;
        body["nostr_event"] = event;
        println!("Nostr event signed (kind 30150)");
    }

    // Audit fix M-13: pred_bps is a JSON number, as_str() returns None.
    // Use as_u64() and convert to string for idempotency key.
    let pred_bps_str = pred_bps.as_u64().map(|n| n.to_string()).unwrap_or_default();
    let idem_key = Ctx::make_idempotency_key(
        "reveal",
        &[market, &pred_bps_str, nonce_val.as_str().unwrap_or("")],
    );
    let resp = ctx.authed_post_json_with_key(
        format!("{}/v1/markets/{market}/reveal", ctx.base),
        &body,
        &idem_key,
    )?;
    println!("Revealed: {} bps", resp["prediction_bps"]);
    println!("Status: {}", resp["status"]);

    // Clean up nonce file
    let _ = std::fs::remove_file(&nonce_file);
    Ok(())
}

/// Sign in with Nostr: create a NIP-98 event, sign it, and submit to /v1/auth/nostr.
fn cmd_auth_nostr(ctx: &Ctx, secret_key_file: Option<&PathBuf>) -> Result<()> {
    use base64::Engine;

    let secret_key_hex = load_nostr_secret_key(secret_key_file, true)?
        .context("auth-nostr requires RAIJU_NOSTR_SECRET_KEY or --secret-key-file")?;
    let sk_bytes = hex::decode(secret_key_hex).context("secret_key must be valid hex")?;
    let sk = secp256k1::SecretKey::from_slice(&sk_bytes)
        .context("secret_key must be a valid 32-byte secp256k1 secret key")?;
    let secp = secp256k1::Secp256k1::new();
    let keypair = secp256k1::Keypair::from_secret_key(&secp, &sk);
    let (xonly, _parity) = keypair.x_only_public_key();
    let pubkey_hex = hex::encode(xonly.serialize());

    let url = format!("{}/v1/auth/nostr", ctx.base);
    let created_at = chrono::Utc::now().timestamp() as u64;

    // Construct the NIP-98 event (kind 27235)
    // Serialize: [0, pubkey, created_at, kind, tags, content]
    let tags = serde_json::json!([["u", url], ["method", "POST"]]);
    let serialized = serde_json::json!([0, pubkey_hex, created_at, 27235, tags, ""]);
    let serialized_str = serde_json::to_string(&serialized)?;

    // Event ID = SHA-256(serialized)
    let mut hasher = Sha256::new();
    hasher.update(serialized_str.as_bytes());
    let event_id: [u8; 32] = hasher.finalize().into();
    let event_id_hex = hex::encode(event_id);

    // Sign the event ID with BIP-340 Schnorr
    let msg = secp256k1::Message::from_digest(event_id);
    let sig = secp.sign_schnorr(&msg, &keypair);
    let sig_hex = hex::encode(sig.serialize());

    // Build the full event JSON
    let event = serde_json::json!({
        "id": event_id_hex,
        "pubkey": pubkey_hex,
        "created_at": created_at,
        "kind": 27235,
        "tags": tags,
        "content": "",
        "sig": sig_hex,
    });

    // Base64 encode and send
    let event_json = serde_json::to_string(&event)?;
    let encoded = base64::prelude::BASE64_STANDARD.encode(event_json.as_bytes());

    let resp: serde_json::Value = ctx
        .client
        .post(&url)
        .header("Authorization", format!("Nostr {encoded}"))
        .header("Content-Type", "application/json")
        .send()?
        .api_error()?
        .json()?;

    if resp["created"].as_bool().unwrap_or(false) {
        println!("New account created!");
        println!("Agent ID: {}", resp["agent_id"]);
        println!("Operator ID: {}", resp["operator_id"]);
        println!("Nostr pubkey: {}", resp["nostr_pubkey"]);
        if let Some(key) = resp["api_key"].as_str() {
            println!("API Key: {key}");
            println!("\nSave this API key. It will not be shown again.");
        }
    } else {
        println!("Signed in with existing account.");
        println!("Agent ID: {}", resp["agent_id"]);
        println!("Nostr pubkey: {}", resp["nostr_pubkey"]);
    }
    Ok(())
}

fn cmd_admin(ctx: &Ctx, action: AdminCommands) -> Result<()> {
    match action {
        AdminCommands::Stats => ctx.get_pretty("/v1/stats")?,

        AdminCommands::VoidMarket { market } => {
            let resp: serde_json::Value = ctx
                .authed_post(format!("{}/v1/markets/{market}/void", ctx.base))
                .send()?
                .api_error()?
                .json()?;
            println!("Market voided: {}", resp["id"]);
        }

        AdminCommands::CreateMarket {
            question,
            criteria,
            oracle_tier,
            pool_entry,
            fee_bps,
            commitment_days,
            resolution_days,
            amm_b,
            category,
            operator,
        } => {
            cmd_admin_create_market(
                ctx,
                &question,
                &criteria,
                &oracle_tier,
                pool_entry,
                fee_bps,
                commitment_days,
                resolution_days,
                amm_b,
                category.as_deref(),
                &operator,
            )?;
        }

        AdminCommands::SeedTemplates { month, operator } => {
            cmd_admin_seed_templates(ctx, &month, &operator)?;
        }

        AdminCommands::DeactivateAgent { agent } => {
            let resp: serde_json::Value = ctx
                .authed_post(format!("{}/v1/agents/{agent}/deactivate", ctx.base))
                .send()?
                .api_error()?
                .json()?;
            println!("Agent deactivated: {}", resp["id"]);
            if let Some(warning) = resp["warning"].as_str() {
                eprintln!("Warning: {warning}");
            }
        }

        AdminCommands::ReactivateAgent { agent } => {
            let resp: serde_json::Value = ctx
                .authed_post(format!("{}/v1/agents/{agent}/reactivate", ctx.base))
                .send()?
                .api_error()?
                .json()?;
            println!("Agent reactivated: {}", resp["id"]);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_admin_create_market(
    ctx: &Ctx,
    question: &str,
    criteria: &str,
    oracle_tier: &str,
    pool_entry: i64,
    fee_bps: i32,
    commitment_days: u32,
    resolution_days: u32,
    amm_b: i64,
    category: Option<&str>,
    operator: &str,
) -> Result<()> {
    let now = chrono::Utc::now();
    let commit_deadline = now + chrono::Duration::days(i64::from(commitment_days));
    let reveal_deadline = commit_deadline + chrono::Duration::hours(1);
    let resolution_date = now + chrono::Duration::days(i64::from(resolution_days));

    let body = serde_json::json!({
        "question": question,
        "resolution_criteria": criteria,
        "oracle_tier": oracle_tier,
        "pool_entry_sats": pool_entry,
        "platform_fee_bps": fee_bps,
        "category": category,
        "commitment_open_at": now.to_rfc3339(),
        "commitment_deadline": commit_deadline.to_rfc3339(),
        "reveal_deadline": reveal_deadline.to_rfc3339(),
        "resolution_date": resolution_date.to_rfc3339(),
        "created_by": operator,
    });
    let resp = ctx.authed_post_json(format!("{}/v1/markets", ctx.base), &body)?;
    let market_id = resp["id"].as_str().context("no market id")?;
    println!("Market created: {market_id}");

    // Init AMM
    #[allow(clippy::cast_possible_truncation)]
    let seed_sats = ((amm_b as f64) * 2.0_f64.ln()).ceil() as i64 + 1;
    let amm_body = serde_json::json!({
        "b_parameter": amm_b,
        "seed_sats": seed_sats,
        "lp_operator_id": operator,
    });
    ctx.authed_post_json(format!("{}/v1/markets/{market_id}/amm", ctx.base), &amm_body)?;
    println!("AMM initialized: b={amm_b}, seed={seed_sats}");

    // Open market
    ctx.authed_post(format!("{}/v1/markets/{market_id}/open", ctx.base)).send()?.api_error()?;
    println!("Market opened and ready for agents.");
    Ok(())
}

fn cmd_admin_seed_templates(ctx: &Ctx, month: &str, operator: &str) -> Result<()> {
    let template_dir = PathBuf::from("data/market-templates");
    if !template_dir.exists() {
        anyhow::bail!("Template directory not found: {}", template_dir.display());
    }

    let parts: Vec<&str> = month.split('-').collect();
    if parts.len() != 2 {
        anyhow::bail!("Month format must be YYYY-MM (e.g., 2026-04)");
    }
    let year = parts[0];
    let month_num: u32 = parts[1].parse().context("invalid month")?;
    let month_names = [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month_name = month_names.get(month_num as usize).unwrap_or(&"Unknown");

    let mut created = 0;
    for entry in std::fs::read_dir(&template_dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = std::fs::read_to_string(entry.path())?;
        let tmpl: serde_json::Value = serde_json::from_str(&content)?;

        let question = tmpl["template"]
            .as_str()
            .unwrap_or("")
            .replace("{MONTH}", month_name)
            .replace("{YEAR}", year)
            .replace("{DATE}", month);

        let criteria = tmpl["resolution_criteria"]
            .as_str()
            .unwrap_or("")
            .replace("{MONTH}", month_name)
            .replace("{YEAR}", year)
            .replace("{DATE}", month);

        let commitment_days = tmpl["commitment_days"].as_u64().unwrap_or(7) as u32;
        let resolution_offset = tmpl["resolution_offset_days"].as_u64().unwrap_or(1);
        let now = chrono::Utc::now();
        let commit_deadline = now + chrono::Duration::days(i64::from(commitment_days));

        let body = serde_json::json!({
            "question": question,
            "resolution_criteria": criteria,
            "oracle_tier": tmpl["oracle_tier"].as_str().unwrap_or("api_median"),
            "pool_entry_sats": tmpl["pool_entry_sats"].as_i64().unwrap_or(10000),
            "platform_fee_bps": tmpl["platform_fee_bps"].as_i64().unwrap_or(250),
            "commitment_open_at": now.to_rfc3339(),
            "commitment_deadline": commit_deadline.to_rfc3339(),
            "reveal_deadline": (commit_deadline + chrono::Duration::hours(1)).to_rfc3339(),
            "resolution_date": (commit_deadline + chrono::Duration::days(resolution_offset as i64)).to_rfc3339(),
            "created_by": operator,
        });
        let resp = ctx.authed_post_json(format!("{}/v1/markets", ctx.base), &body)?;

        let market_id = resp["id"].as_str().unwrap_or("?");
        println!("  Created: {question} [{market_id}]");
        created += 1;
    }
    println!("\nSeeded {created} markets from templates.");
    Ok(())
}

/// Returns the nonce file path for an agent's market commitment.
/// Path: ~/.raiju/nonces/<AGENT_ID>/<MARKET_ID>.json
/// Agent-namespaced to prevent collisions when running multiple agents.
fn nonce_path(agent_id: &str, market_id: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".raiju").join("nonces").join(agent_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{market_id}.json")))
}

/// Extension trait to add `error_for_status` with readable error messages.
trait ResponseExt {
    fn api_error(self) -> Result<reqwest::blocking::Response>;
}

impl ResponseExt for reqwest::blocking::Response {
    fn api_error(self) -> Result<reqwest::blocking::Response> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            let body = self.text().unwrap_or_default();
            // Try to extract error message from JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(msg) = json["error"].as_str() {
                    anyhow::bail!("{status}: {msg}");
                }
            }
            anyhow::bail!("{status}: {body}");
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Compute commitment hash using the CLI's logic (must match raiju-types and Python SDK).
    fn cli_compute_hash(prediction_bps: u16, nonce: &[u8]) -> String {
        let pred_bytes = (prediction_bps as i32).to_be_bytes();
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN_SEPARATOR);
        hasher.update(pred_bytes);
        hasher.update(nonce);
        hex::encode(hasher.finalize())
    }

    /// H-10 + H-14: Verify CLI hash matches the shared test vectors from docs/test-vectors/commitment-hash.json.
    ///
    /// These vectors are also verified by raiju-types and the Python SDK.
    /// If this test fails, the CLI and server would diverge on commit-reveal.
    #[test]
    fn test_audit_h10_commitment_hash_test_vectors() {
        // Vector 1: 72% forecast, 0xAA nonce
        let nonce = vec![0xAAu8; 32];
        assert_eq!(
            cli_compute_hash(7200, &nonce),
            "e37bd54454dd1c34d39175ad705ba757dcf1ff7bb0d88ac44b2343d976b214e2"
        );

        // Vector 2: 0% forecast, zero nonce
        let nonce = vec![0x00u8; 32];
        assert_eq!(
            cli_compute_hash(0, &nonce),
            "b947e135b598a4c88194bd667d7c7272c62d6dc0330bfcd539b260c5cffb6c49"
        );

        // Vector 3: 100% forecast, 0xFF nonce
        let nonce = vec![0xFFu8; 32];
        assert_eq!(
            cli_compute_hash(10000, &nonce),
            "f481e72a1b60675d121415e88ab1fc410353f578bc705c991532fec685b470a8"
        );

        // Vector 4: 50% forecast, 0xDEADBEEF pattern nonce
        let nonce = hex::decode("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
            .unwrap();
        assert_eq!(
            cli_compute_hash(5000, &nonce),
            "3a40b116b4687e514e05c98e6c139385934227728ca2dc1c26ad0fe4916e0a44"
        );
    }

    /// H-14: Domain separator is included in hash computation.
    #[test]
    fn test_audit_h14_domain_separator_included() {
        let nonce = vec![0xAA; 32];
        let with_separator = cli_compute_hash(7200, &nonce);

        // Compute WITHOUT DOMAIN_SEPARATOR (the bug from C-4)
        let pred_bytes = (7200i32).to_be_bytes();
        let mut hasher = Sha256::new();
        // No DOMAIN_SEPARATOR prefix
        hasher.update(pred_bytes);
        hasher.update(&nonce);
        let without_separator = hex::encode(hasher.finalize());

        assert_ne!(
            with_separator, without_separator,
            "hash must differ with/without domain separator"
        );
    }

    /// H-14: Prediction encoding is big-endian i32.
    #[test]
    fn test_audit_h14_prediction_encoding() {
        // 7200 as i32 BE = [0x00, 0x00, 0x1C, 0x20]
        let pred_bytes = (7200i32).to_be_bytes();
        assert_eq!(pred_bytes, [0x00, 0x00, 0x1C, 0x20]);

        // 0 as i32 BE = [0x00, 0x00, 0x00, 0x00]
        let pred_bytes = (0i32).to_be_bytes();
        assert_eq!(pred_bytes, [0x00, 0x00, 0x00, 0x00]);

        // 10000 as i32 BE = [0x00, 0x00, 0x27, 0x10]
        let pred_bytes = (10000i32).to_be_bytes();
        assert_eq!(pred_bytes, [0x00, 0x00, 0x27, 0x10]);
    }

    #[test]
    fn test_auth_nostr_accepts_secret_key_file_instead_of_raw_argv_secret() {
        let parsed =
            Cli::try_parse_from(["raiju", "auth-nostr", "--secret-key-file", "/tmp/nostr.key"]);

        assert!(parsed.is_ok(), "CLI should support a safer non-argv secret source for auth-nostr");
    }

    #[test]
    fn test_audit_protected_write_includes_idempotency_key() {
        let ctx = Ctx {
            client: reqwest::blocking::Client::new(),
            base: "http://localhost:3001".to_string(),
            auth: None,
        };

        let req = ctx
            .authed_post("http://localhost:3001/v1/markets/123/deposit".to_string())
            .build()
            .unwrap();

        assert!(
            req.headers().contains_key("Idempotency-Key"),
            "protected CLI writes must include an Idempotency-Key header"
        );
    }
}
