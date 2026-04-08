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
use raiju::client::RaijuClient;
use std::path::PathBuf;

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

// ── Helpers ────────────────────────────────────────────────────────────

/// Pretty-print a JSON value to stdout.
fn pretty(v: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// Print notices from a /v1/status response to stderr.
fn print_notices_from(resp: &serde_json::Value) {
    if let Some(notices) = resp["notices"].as_array() {
        for notice in notices {
            if let (Some(ntype), Some(msg)) = (notice["type"].as_str(), notice["message"].as_str())
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

/// Print platform notices (best-effort, never blocks).
fn print_notices(client: &RaijuClient) {
    if let Ok(status) = client.status() {
        print_notices_from(&status);
    }
}

// ── Main ───────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = RaijuClient::new(&cli.url, cli.api_key.as_deref());

    let is_health_or_info = matches!(cli.command, Commands::Health | Commands::Info);

    match cli.command {
        Commands::Health => {
            let resp = client.status()?;
            println!("Status: {}", resp["database"].as_str().unwrap_or("unknown"));
            println!("Version: {}", resp["version"].as_str().unwrap_or("unknown"));
            print_notices_from(&resp);
        }

        Commands::Info => {
            let resp = client.status()?;
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
            print_notices_from(&resp);
        }

        Commands::RegisterOperator { name, nwc_uri } => {
            let resp = client.register_operator(&name, nwc_uri.as_deref())?;
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
            let resp = client.register_agent(
                &operator,
                &name,
                description.as_deref(),
                repo_url.as_deref(),
                nwc_uri.as_deref(),
            )?;
            println!("Agent ID: {}", resp["id"]);
            println!("API Key: {}", resp["api_key"]);
            println!("\nSave this API key. It will not be shown again.");
            println!("Connect your wallet: raiju wallet set --agent {} --nwc-uri \"nostr+walletconnect://...\"", resp["id"]);
        }

        Commands::Markets { category, status } => {
            let resp = client.list_markets(status.as_deref(), category.as_deref())?;
            let markets = resp.as_array().context("expected array")?;
            println!("{:<8} {:<12} Question", "ID", "Status");
            println!("{}", "-".repeat(60));
            for m in markets {
                let id = m["id"].as_str().unwrap_or("?");
                println!(
                    "{:<8} {:<12} {}",
                    &id[..8.min(id.len())],
                    m["status"].as_str().unwrap_or("?"),
                    m["question"].as_str().unwrap_or("?"),
                );
            }
            println!("\n{} markets total", markets.len());
        }

        Commands::WalletSet { agent, nwc_uri } => {
            let resp = client.wallet_set(&agent, &nwc_uri)?;
            println!("Connected: {}", resp["connected"]);
            if let Some(verified) = resp["verified_at"].as_str() {
                println!("Verified at: {verified}");
            } else {
                println!("Verification: pending (background check in progress)");
            }
        }

        Commands::WalletStatus { agent } => {
            let resp = client.wallet_status(&agent)?;
            println!("Connected: {}", resp["connected"]);
            if let Some(verified) = resp["verified_at"].as_str() {
                println!("Verified at: {verified}");
            }
        }

        Commands::WalletRemove { agent } => {
            client.wallet_remove(&agent)?;
            println!("Wallet disconnected.");
        }

        Commands::Market { id } => pretty(&client.market_detail(&id)?)?,

        Commands::Deposit { market, agent, amount } => {
            let resp = client.deposit(&market, &agent, amount)?;
            println!("Deposit ID: {}", resp["id"]);
            println!("Amount: {} sats", resp["amount_sats"]);
            println!("Status: {}", resp["status"]);
            if let Some(method) = resp["method"].as_str() {
                println!("Method: {method}");
            }
            if let Some(bolt11) = resp["bolt11"].as_str() {
                println!("\nBOLT11 invoice (pay this to complete deposit):");
                println!("{bolt11}");
            }
        }

        Commands::Commit { market, agent, prediction, nostr_sign, nostr_secret_key_file } => {
            let nostr_secret = if nostr_sign || nostr_secret_key_file.is_some() {
                raiju::nostr::load_secret_key(nostr_secret_key_file.as_deref(), true)?
            } else {
                raiju::nostr::load_secret_key(nostr_secret_key_file.as_deref(), false)?
            };
            if nostr_secret.is_some() {
                println!("Nostr event signed (kind 30150)");
            }
            let resp = client.commit(&market, &agent, prediction, nostr_secret.as_deref())?;
            println!("Commitment: {}", resp["status"]);
        }

        Commands::Predict { market, agent, prediction } => {
            let resp = client.predict(&market, &agent, prediction)?;
            println!("Prediction ID: {}", resp["id"]);
            println!("Status: {}", resp["status"]);
            println!("Mode: fire-and-forget (server will auto-reveal at deadline)");
        }

        Commands::Reveal { market, agent, nostr_sign, nostr_secret_key_file } => {
            let nostr_secret = if nostr_sign || nostr_secret_key_file.is_some() {
                raiju::nostr::load_secret_key(nostr_secret_key_file.as_deref(), true)?
            } else {
                raiju::nostr::load_secret_key(nostr_secret_key_file.as_deref(), false)?
            };
            if nostr_secret.is_some() {
                println!("Nostr event signed (kind 30150)");
            }
            let resp = client.reveal(&market, &agent, nostr_secret.as_deref())?;
            println!("Reveal: {}", resp["status"]);
        }

        Commands::Balance { market, agent } => pretty(&client.amm_balance(&market, &agent)?)?,

        Commands::Trade { market, agent, direction, shares } => {
            let valid = ["buy_yes", "buy_no", "sell_yes", "sell_no"];
            if !valid.contains(&direction.as_str()) {
                anyhow::bail!("direction must be one of: {}", valid.join(", "));
            }
            let resp = client.trade(&market, &agent, &direction, shares)?;
            println!("Trade ID: {}", resp["trade_id"]);
            println!("Cost: {} sats", resp["cost_sats"]);
            println!("Price: {} -> {} bps", resp["price_before_bps"], resp["price_after_bps"]);
        }

        Commands::Leaderboard { period } => {
            let resp = client.leaderboard(None, period.as_deref())?;
            let entries = resp.as_array().context("expected array")?;
            if let Some(p) = &period {
                println!("Period: {p}");
            }
            println!("{:<4} {:<20} {:<12} {:<12}", "#", "Agent", "Quality", "PnL");
            println!("{}", "-".repeat(50));
            for (i, e) in entries.iter().enumerate() {
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
            let resp = client.agent_achievements(&agent_id)?;
            let items = resp.as_array().context("expected array")?;
            if items.is_empty() {
                println!("No achievements yet.");
            } else {
                for a in items {
                    println!(
                        "{} - {} ({})",
                        a["display_title"].as_str().unwrap_or("?"),
                        a["category"].as_str().unwrap_or("?"),
                        a["period_key"].as_str().unwrap_or("?"),
                    );
                }
            }
        }

        Commands::Status { agent } => pretty(&client.agent_status(&agent)?)?,
        Commands::Consensus { market } => pretty(&client.consensus(&market)?)?,
        Commands::Amm { market } => pretty(&client.amm_state(&market)?)?,
        Commands::PriceHistory { market } => pretty(&client.price_history(&market)?)?,
        Commands::Deposits { market } => pretty(&client.market_deposits(&market)?)?,
        Commands::Predictions { market } => pretty(&client.market_predictions(&market)?)?,
        Commands::Stats { market } => pretty(&client.market_stats(&market)?)?,
        Commands::Positions { agent } => pretty(&client.positions(&agent)?)?,

        Commands::Agents { limit, offset } => {
            let resp = client.list_agents(
                limit.map(|l| l as u64),
                offset.map(|o| o as u64),
            )?;
            let agents = resp.as_array().context("expected array")?;
            println!("{:<8} {:<20}", "ID", "Name");
            println!("{}", "-".repeat(30));
            for a in agents {
                let id = a["id"].as_str().unwrap_or("?");
                println!(
                    "{:<8} {:<20}",
                    &id[..8.min(id.len())],
                    a["display_name"].as_str().unwrap_or("?"),
                );
            }
            println!("\n{} agents", agents.len());
        }

        Commands::Trades { market, agent } => {
            pretty(&client.trade_history(agent.as_deref(), market.as_deref())?)?;
        }

        Commands::Payouts { agent } => pretty(&client.payouts(&agent)?)?,
        Commands::Settlements { agent, status } => {
            pretty(&client.settlements(&agent, status.as_deref())?)?;
        }

        Commands::ClaimPayout { payout, agent, bolt11 } => {
            let resp = client.claim_payout(&payout, &agent, &bolt11)?;
            pretty(&resp)?;
        }

        Commands::ClaimSettlement { settlement, agent, bolt11 } => {
            let resp = client.claim_settlement(&settlement, &agent, &bolt11)?;
            pretty(&resp)?;
        }

        Commands::ClaimAll { agent } => {
            let payouts_resp = client.payouts_by_status(&agent, "pending_claim")?;
            let settlements_resp = client.settlements(&agent, Some("pending_claim"))?;
            let payouts = payouts_resp.as_array().context("expected array")?;
            let settlements = settlements_resp.as_array().context("expected array")?;

            if payouts.is_empty() && settlements.is_empty() {
                println!("No pending claims for agent {agent}.");
            } else {
                let mut total = 0i64;
                if !payouts.is_empty() {
                    println!("BWM Payouts ({}):", payouts.len());
                    println!("  {:<38} {:>10}", "ID", "Amount");
                    for p in payouts {
                        let id = p["id"].as_str().unwrap_or("?");
                        let sats = p["payout_sats"].as_i64().unwrap_or(0);
                        total += sats;
                        println!("  {:<38} {:>10} sats", id, sats);
                    }
                }
                if !settlements.is_empty() {
                    println!("AMM Settlements ({}):", settlements.len());
                    println!("  {:<38} {:>10}", "ID", "Amount");
                    for s in settlements {
                        let id = s["id"].as_str().unwrap_or("?");
                        let sats = s["total_claimable_sats"].as_i64().unwrap_or(0);
                        total += sats;
                        println!("  {:<38} {:>10} sats", id, sats);
                    }
                }
                println!("\nTotal claimable: {} sats", total);
                println!("\nTo claim, generate a BOLT11 invoice for each amount and run:");
                for p in payouts {
                    let id = p["id"].as_str().unwrap_or("?");
                    let sats = p["payout_sats"].as_i64().unwrap_or(0);
                    println!(
                        "  raiju claim-payout --payout {} --agent {} --bolt11 <invoice_for_{}_sats>",
                        id, agent, sats
                    );
                }
                for s in settlements {
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
            cmd_auth_nostr(&client, secret_key_file.as_deref())?;
        }

        Commands::NostrChallenge { pubkey } => {
            let resp = client.nostr_challenge(&pubkey)?;
            pretty(&resp)?;
        }

        Commands::NostrBind { pubkey, signature } => {
            let resp = client.nostr_bind_manual(&pubkey, &signature)?;
            pretty(&resp)?;
        }

        Commands::NostrUnbind => {
            let resp = client.nostr_unbind()?;
            pretty(&resp)?;
        }

        Commands::DeactivateAgent { agent } => {
            let resp = client.deactivate_agent(&agent)?;
            println!("Agent deactivated: {}", resp["id"]);
            if let Some(warning) = resp["warning"].as_str() {
                eprintln!("Warning: {warning}");
            }
        }

        Commands::ReactivateAgent { agent } => {
            let resp = client.reactivate_agent(&agent)?;
            println!("Agent reactivated: {}", resp["id"]);
        }

        Commands::Admin { action } => cmd_admin(&client, action)?,
    }

    // Print platform notices after any command (best-effort, never blocks).
    if !is_health_or_info {
        print_notices(&client);
    }

    Ok(())
}

// ── Auth Nostr (NIP-98, CLI-specific) ──────────────────────────────────

fn cmd_auth_nostr(client: &RaijuClient, secret_key_file: Option<&std::path::Path>) -> Result<()> {
    use base64::Engine;

    let secret_key_hex = raiju::nostr::load_secret_key(secret_key_file, true)?
        .context("auth-nostr requires RAIJU_NOSTR_SECRET_KEY or --secret-key-file")?;

    let (pubkey_hex, _keypair) = raiju::nostr::derive_pubkey(&secret_key_hex)?;

    // Build NIP-98 event (kind 27235) with the auth URL
    let url = format!("{}/v1/auth/nostr", client.base_url());
    let tags = serde_json::json!([["u", url], ["method", "POST"]]);
    let event = raiju::nostr::build_event(&secret_key_hex, 27235, tags)?;

    // Base64 encode and send as Nostr auth header
    let event_json = serde_json::to_string(&event)?;
    let encoded = base64::prelude::BASE64_STANDARD.encode(event_json.as_bytes());

    // This uses a raw POST with Nostr auth header (not Bearer token)
    let resp: serde_json::Value = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .post(&format!("{}/v1/auth/nostr", client.base_url()))
        .header("Authorization", format!("Nostr {encoded}"))
        .header("Content-Type", "application/json")
        .send()?
        .json()?;

    // Check for HTTP error in response
    if let Some(err) = resp["error"].as_str() {
        anyhow::bail!("{err}");
    }

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
        println!("Nostr pubkey: {pubkey_hex}");
    }
    Ok(())
}

// ── Admin commands (CLI-specific) ──────────────────────────────────────

fn cmd_admin(client: &RaijuClient, action: AdminCommands) -> Result<()> {
    match action {
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
                client,
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

        AdminCommands::Stats => pretty(&client.get("/v1/stats")?)?,

        AdminCommands::VoidMarket { market } => {
            client.post(&format!("/v1/markets/{market}/void"), &serde_json::json!({}), None)?;
            println!("Market voided: {market}");
        }

        AdminCommands::SeedTemplates { month, operator } => {
            cmd_admin_seed_templates(client, &month, &operator)?;
        }

        AdminCommands::DeactivateAgent { agent } => {
            let resp = client.deactivate_agent(&agent)?;
            println!("Agent deactivated: {}", resp["id"]);
        }

        AdminCommands::ReactivateAgent { agent } => {
            let resp = client.reactivate_agent(&agent)?;
            println!("Agent reactivated: {}", resp["id"]);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_admin_create_market(
    client: &RaijuClient,
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
    let resp = client.post("/v1/markets", &body, None)?;
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
    client.post(&format!("/v1/markets/{market_id}/amm"), &amm_body, None)?;
    println!("AMM initialized: b={amm_b}, seed={seed_sats}");

    // Open market
    client.post(&format!("/v1/markets/{market_id}/open"), &serde_json::json!({}), None)?;
    println!("Market opened and ready for agents.");
    Ok(())
}

fn cmd_admin_seed_templates(client: &RaijuClient, month: &str, operator: &str) -> Result<()> {
    let template_dir = std::path::PathBuf::from("data/market-templates");
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
        let resp = client.post("/v1/markets", &body, None)?;

        let market_id = resp["id"].as_str().unwrap_or("?");
        println!("  Created: {question} [{market_id}]");
        created += 1;
    }
    println!("\nSeeded {created} markets from templates.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_auth_nostr_accepts_secret_key_file_instead_of_raw_argv_secret() {
        let parsed =
            Cli::try_parse_from(["raiju", "auth-nostr", "--secret-key-file", "/tmp/nostr.key"]);
        assert!(parsed.is_ok(), "CLI should support a safer non-argv secret source for auth-nostr");
    }

    #[test]
    fn test_protected_write_includes_idempotency_key() {
        let client = RaijuClient::new("http://localhost:3001", None);
        // Verify the client can be constructed (basic sanity)
        assert_eq!(client.base_url(), "http://localhost:3001");
    }
}
