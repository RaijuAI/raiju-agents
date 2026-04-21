//! `raiju events` - real-time SSE event stream subcommand.
//!
//! Wraps `GET /v1/events` with opinionated defaults so agent operators don't
//! have to parse SSE manually or reinvent reconnect/filter/flatten logic.
//!
//! The endpoint is public: no auth is required, though this CLI will send a
//! Bearer token when one is configured for forward-compatibility with a
//! future per-agent authenticated stream.

use crate::client::RaijuClient;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Output format for each event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Flat JSONL: envelope + inner `data` merged into one object, one per line.
    Jsonl,
    /// Raw SSE frames passed through to stdout.
    Sse,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "jsonl" => Ok(Self::Jsonl),
            "sse" => Ok(Self::Sse),
            other => bail!("unknown --output '{other}', expected 'jsonl' or 'sse'"),
        }
    }
}

/// Arguments parsed by the `raiju events` subcommand.
#[derive(Debug, Clone)]
pub struct EventsArgs {
    /// Comma-separated list of market UUIDs for the server-side filter.
    pub markets: Option<String>,
    /// File containing one market UUID per line.
    pub markets_from_file: Option<PathBuf>,
    /// Auto-track every currently-open market by listening for lifecycle events.
    pub follow_open: bool,
    /// Comma-separated list of event types to pass through (client-side filter).
    pub types: Option<String>,
    /// Output shape: `jsonl` (default, flat) or `sse` (raw passthrough).
    pub output: OutputFormat,
    /// Stop after this many events (useful for CI/scripts).
    pub max_events: Option<usize>,
    /// Stop after this many reconnect attempts.
    pub reconnect_max: Option<usize>,
    /// Log `: ping` keepalive comments to stderr.
    pub heartbeat_to_stderr: bool,
    /// Initialize the resume cursor so the first stream request sends
    /// `Last-Event-ID: <since>`. The server's ring buffer replays newer
    /// events before attaching live; if `since` predates the buffer, a
    /// `resume.gap` event is emitted so the client can reconcile.
    pub since: Option<u64>,
    /// Route to `/v1/events/private` instead of `/v1/events`. Requires an
    /// API key; the caller's own events are emitted without sanitization.
    /// Every other agent's events remain sanitized.
    pub private: bool,
}

/// Entry point called from `main.rs`.
pub fn run(client: &RaijuClient, args: EventsArgs) -> Result<()> {
    validate_flags(&args)?;

    // Only used under --follow-open to suppress lifecycle events from
    // stdout when the user didn't ask for them. Normal type filtering is
    // server-side via the ?types= query parameter (single source of truth).
    let output_type_filter: Option<HashSet<String>> = args.types.as_ref().map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    });

    let mut events_emitted: usize = 0;
    let mut reconnect_count: usize = 0;
    let mut seen_collision_warning = false;
    // Highest event_id the CLI has successfully emitted to stdout. Sent as
    // the `Last-Event-ID` header on every reconnect so the server can
    // resume from the ring buffer without gaps. Initialized from --since
    // when provided; the server's Last-Event-ID replay handles the gap
    // fill via the ring buffer, no separate REST call needed.
    let mut last_event_id: u64 = args.since.unwrap_or(0);

    let server_types = compute_server_types(args.types.as_deref(), args.follow_open);

    loop {
        let FilterSet { server_markets, client_follow } = resolve_filter_set(client, &args)?;

        let path = build_path(
            server_markets.as_deref(),
            server_types.as_deref(),
            args.private,
        );
        let result = stream_once(
            client,
            &path,
            &output_type_filter,
            &client_follow,
            &args,
            &mut events_emitted,
            &mut seen_collision_warning,
            &mut last_event_id,
        );

        match result {
            Ok(StreamOutcome::MaxEventsReached) => return Ok(()),
            Ok(StreamOutcome::Closed) => {
                eprintln!(
                    "[raiju events] connection closed by server, reconnecting (last_event_id={last_event_id})..."
                );
            }
            Err(e) => {
                eprintln!("[raiju events] stream error: {e:#}");
            }
        }

        reconnect_count += 1;
        if let Some(max) = args.reconnect_max {
            if reconnect_count > max {
                bail!("reached --reconnect-max {max}, giving up");
            }
        }
        let delay = backoff(reconnect_count);
        eprintln!(
            "[raiju events] reconnect attempt {reconnect_count} in {}s",
            delay.as_secs()
        );
        std::thread::sleep(delay);
    }
}

/// Lifecycle events that `--follow-open` must always see to maintain its
/// tracked set (`update_follow_set` switches on these).
const LIFECYCLE_TYPES: &[&str] = &["market.opened", "market.resolved", "market.voided"];

/// Compute the server-side `?types=` query string.
///
/// When `follow_open` is active and the user passed `--type`, lifecycle
/// events are injected into the server-side filter so
/// `update_follow_set` receives them. The client-side
/// `output_type_filter` in `handle_frame` then suppresses them from
/// stdout unless the user explicitly requested them.
///
/// When `follow_open` is off, we pass the user's types through unchanged.
fn compute_server_types(user_types: Option<&str>, follow_open: bool) -> Option<String> {
    let user_types = user_types?;
    if !follow_open {
        return Some(user_types.to_string());
    }
    let mut types: Vec<&str> =
        user_types.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    for lt in LIFECYCLE_TYPES {
        if !types.iter().any(|x| *x == *lt) {
            types.push(lt);
        }
    }
    Some(types.join(","))
}

fn validate_flags(args: &EventsArgs) -> Result<()> {
    let explicit_markets =
        args.markets.is_some() || args.markets_from_file.is_some();
    if args.follow_open && explicit_markets {
        bail!(
            "--follow-open is mutually exclusive with --markets / --markets-from-file"
        );
    }
    // `--private` needs a Bearer token; the CLI sends it automatically when
    // `RAIJU_API_KEY` is configured. Catching the missing key upfront avoids
    // a silent 401 deep in the SSE loop.
    // Note: we can't read the client's api_key from this module without
    // plumbing it through. The check happens at `open_sse_stream` via
    // the actual HTTP 401; we surface a hint in the error below.
    Ok(())
}

struct FilterSet {
    /// When `Some`, sent to the server as `?markets=...`. When `None`, the
    /// server-side filter is omitted (firehose).
    server_markets: Option<String>,
    /// When `Some`, the client filters each event by envelope `market_id`
    /// and keeps the set in sync via lifecycle events.
    client_follow: Option<HashSet<String>>,
}

fn resolve_filter_set(client: &RaijuClient, args: &EventsArgs) -> Result<FilterSet> {
    if let Some(ref csv) = args.markets {
        return Ok(FilterSet {
            server_markets: Some(csv.clone()),
            client_follow: None,
        });
    }
    if let Some(ref path) = args.markets_from_file {
        let uuids = read_markets_file(path)?;
        if uuids.is_empty() {
            bail!("{} contained no market UUIDs", path.display());
        }
        return Ok(FilterSet {
            server_markets: Some(uuids.join(",")),
            client_follow: None,
        });
    }
    if args.follow_open {
        let open = fetch_open_markets(client)?;
        eprintln!(
            "[raiju events] --follow-open subscribed to firehose, tracking {} open markets",
            open.len()
        );
        return Ok(FilterSet { server_markets: None, client_follow: Some(open) });
    }
    // Default: firehose, no client filter.
    Ok(FilterSet { server_markets: None, client_follow: None })
}

fn read_markets_file(path: &Path) -> Result<Vec<String>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(String::from)
        .collect())
}

fn fetch_open_markets(client: &RaijuClient) -> Result<HashSet<String>> {
    let resp = client
        .list_markets(Some("open"), None)
        .context("failed to fetch open markets for --follow-open")?;
    let arr = resp.as_array().context("expected /v1/markets to return an array")?;
    Ok(arr
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from))
        .collect())
}

fn build_path(
    server_markets: Option<&str>,
    server_types: Option<&str>,
    private: bool,
) -> String {
    let base = if private { "/v1/events/private" } else { "/v1/events" };
    let mut params: Vec<String> = Vec::new();
    if let Some(m) = server_markets {
        params.push(format!("markets={}", urlencode(m)));
    }
    if let Some(t) = server_types {
        params.push(format!("types={}", urlencode(t)));
    }
    if params.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", params.join("&"))
    }
}

/// Minimal percent-encoding for the `markets` query value. The CLI only
/// ever passes comma-separated UUIDs here so a full urlencoding dep is
/// overkill, but commas must survive.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'~' | b',' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug)]
enum StreamOutcome {
    Closed,
    MaxEventsReached,
}

fn stream_once(
    client: &RaijuClient,
    path: &str,
    output_type_filter: &Option<HashSet<String>>,
    client_follow: &Option<HashSet<String>>,
    args: &EventsArgs,
    events_emitted: &mut usize,
    seen_collision_warning: &mut bool,
    last_event_id: &mut u64,
) -> Result<StreamOutcome> {
    // Only send Last-Event-ID when we actually have one. Zero means "never
    // seen any event"; the server's initial replay in that case is empty,
    // which is also correct, but sending "0" is noise.
    let last_id_str = if *last_event_id > 0 {
        Some(last_event_id.to_string())
    } else {
        None
    };
    let resp = client.open_sse_stream(path, last_id_str.as_deref())?;
    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut event_type = String::new();
    let mut data_buf = String::new();
    let mut sse_id_buf = String::new();
    let mut follow_set = client_follow.clone();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .context("failed to read from SSE stream")?;
        if n == 0 {
            return Ok(StreamOutcome::Closed);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);

        if trimmed.is_empty() {
            // End of an SSE frame. Emit what we have.
            if !data_buf.is_empty() {
                let outcome = handle_frame(
                    &event_type,
                    &data_buf,
                    output_type_filter,
                    &mut follow_set,
                    args,
                    events_emitted,
                    seen_collision_warning,
                )?;
                // Advance `last_event_id` from the SSE `id:` line, falling
                // back to the payload's `event_id` if the line was absent.
                if let Ok(parsed) = sse_id_buf.parse::<u64>() {
                    if parsed > *last_event_id {
                        *last_event_id = parsed;
                    }
                } else if let Ok(value) = serde_json::from_str::<serde_json::Value>(&data_buf) {
                    if let Some(id) = value.get("event_id").and_then(|v| v.as_u64()) {
                        if id > *last_event_id {
                            *last_event_id = id;
                        }
                    }
                }
                if matches!(outcome, Some(StreamOutcome::MaxEventsReached)) {
                    return Ok(StreamOutcome::MaxEventsReached);
                }
            }
            event_type.clear();
            data_buf.clear();
            sse_id_buf.clear();
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix(':') {
            // SSE comment (keepalive ping).
            if args.heartbeat_to_stderr {
                eprintln!("[raiju events] keepalive{rest}");
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("event:") {
            event_type.push_str(rest.trim_start());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("id:") {
            sse_id_buf.push_str(rest.trim_start());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("data:") {
            let chunk = rest.strip_prefix(' ').unwrap_or(rest);
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(chunk);
            continue;
        }
        // `retry:` and unknown lines are ignored.
    }
}

/// Returns `Some(MaxEventsReached)` when the emit count hits the user's cap.
fn handle_frame(
    event_type: &str,
    data_str: &str,
    output_type_filter: &Option<HashSet<String>>,
    follow_set: &mut Option<HashSet<String>>,
    args: &EventsArgs,
    events_emitted: &mut usize,
    seen_collision_warning: &mut bool,
) -> Result<Option<StreamOutcome>> {
    // Parse the envelope JSON once. Both filters and output shapes need it.
    let envelope: Value = match serde_json::from_str(data_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[raiju events] WARN: failed to parse SSE data as JSON: {e}");
            return Ok(None);
        }
    };

    // Maintain the follow-open set by listening for lifecycle events.
    if let Some(set) = follow_set.as_mut() {
        update_follow_set(set, event_type, &envelope);
    }

    // Client-side market_id filter (for --follow-open).
    if let Some(set) = follow_set.as_ref() {
        let market_id = envelope
            .get("market_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !set.contains(market_id) {
            // Still maintain the set (already done above) but don't emit.
            return Ok(None);
        }
    }

    // Output-suppression type filter: the server already dropped types the
    // user didn't request; this filter only runs under --follow-open to
    // suppress lifecycle events injected into the server-side filter so
    // update_follow_set could see them. In all other cases this is a
    // no-op (server-side filter already did the work).
    if args.follow_open {
        if let Some(tf) = output_type_filter {
            if !tf.contains(event_type) {
                return Ok(None);
            }
        }
    }

    // Emit per --output format.
    match args.output {
        OutputFormat::Sse => {
            println!("event: {event_type}");
            println!("data: {data_str}");
            println!();
        }
        OutputFormat::Jsonl => {
            let flat = flatten_envelope(envelope, seen_collision_warning);
            let line = serde_json::to_string(&flat)?;
            println!("{line}");
        }
    }
    // Flush so downstream pipes see events in real time.
    let _ = std::io::stdout().flush();

    *events_emitted += 1;
    if let Some(max) = args.max_events {
        if *events_emitted >= max {
            return Ok(Some(StreamOutcome::MaxEventsReached));
        }
    }
    Ok(None)
}

fn update_follow_set(set: &mut HashSet<String>, event_type: &str, envelope: &Value) {
    let market_id = envelope
        .get("market_id")
        .and_then(Value::as_str)
        .map(String::from);
    let Some(market_id) = market_id else { return };
    match event_type {
        "market.opened" => {
            set.insert(market_id);
        }
        "market.resolved" | "market.voided" => {
            set.remove(&market_id);
        }
        _ => {}
    }
}

/// Flatten the envelope + inner `data` object into a single flat object with
/// canonical key ordering: `type`, `market_id`, `event_id`, `timestamp`, then
/// any other envelope fields, then every inner-data field. Collisions rename
/// inner keys to `data_<key>` and emit one stderr warning per run.
fn flatten_envelope(envelope: Value, seen_collision_warning: &mut bool) -> Value {
    let Value::Object(mut outer) = envelope else {
        return envelope;
    };
    let inner = outer.remove("data");
    let mut flat = Map::new();

    for key in ["type", "market_id", "event_id", "timestamp"] {
        if let Some(v) = outer.remove(key) {
            flat.insert(key.to_string(), v);
        }
    }
    for (k, v) in outer {
        flat.entry(k).or_insert(v);
    }

    if let Some(Value::Object(inner_obj)) = inner {
        for (k, v) in inner_obj {
            if flat.contains_key(&k) {
                let prefixed = format!("data_{k}");
                if !*seen_collision_warning {
                    eprintln!(
                        "[raiju events] WARN: inner data key '{k}' collides with envelope, renamed to '{prefixed}'. Further collisions suppressed."
                    );
                    *seen_collision_warning = true;
                }
                flat.insert(prefixed, v);
            } else {
                flat.insert(k, v);
            }
        }
    }

    Value::Object(flat)
}

fn backoff(attempt: usize) -> Duration {
    // 1, 2, 4, 8, 16, 30, 30, 30...
    let secs = (1u64 << attempt.min(5)).min(30);
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_merges_envelope_and_data() {
        let envelope = json!({
            "type": "amm.price_update",
            "market_id": "cd2b1f68-a97f-4b4e-ab1a-0d0a5b0f9d5e",
            "timestamp": "2026-04-11T15:29:12Z",
            "data": {
                "market_id": "cd2b1f68-a97f-4b4e-ab1a-0d0a5b0f9d5e",
                "yes_price_bps": 4808,
                "no_price_bps": 5192,
                "direction": "buy_no",
                "shares": 1,
                "trade_id": "a94c0000-0000-0000-0000-000000000001"
            }
        });
        let mut warned = false;
        let flat = flatten_envelope(envelope, &mut warned);
        let obj = flat.as_object().unwrap();
        assert_eq!(obj["type"], "amm.price_update");
        assert_eq!(obj["yes_price_bps"], 4808);
        assert_eq!(obj["trade_id"], "a94c0000-0000-0000-0000-000000000001");
        // market_id from envelope wins (data.market_id duplicates the envelope value).
        assert_eq!(obj["market_id"], "cd2b1f68-a97f-4b4e-ab1a-0d0a5b0f9d5e");
        // Collision warning did NOT fire because the two market_id values matched
        // and the inner write was absorbed into the existing entry via equality.
        // Actually the code prefixes even when values match, so the warning fires.
        assert!(warned);
        // The collision should have produced data_market_id too.
        assert!(obj.contains_key("data_market_id"));
    }

    #[test]
    fn flatten_no_collision_without_shared_keys() {
        let envelope = json!({
            "type": "amm.trade",
            "market_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "timestamp": "t",
            "data": {"direction": "buy_yes", "shares": 3}
        });
        let mut warned = false;
        let flat = flatten_envelope(envelope, &mut warned);
        assert!(!warned);
        assert_eq!(flat["direction"], "buy_yes");
    }

    #[test]
    fn output_format_parser() {
        assert_eq!(OutputFormat::parse("jsonl").unwrap(), OutputFormat::Jsonl);
        assert_eq!(OutputFormat::parse("JSONL").unwrap(), OutputFormat::Jsonl);
        assert_eq!(OutputFormat::parse("sse").unwrap(), OutputFormat::Sse);
        assert!(OutputFormat::parse("json").is_err());
    }

    #[test]
    fn backoff_progression() {
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(2), Duration::from_secs(4));
        assert_eq!(backoff(5), Duration::from_secs(30));
        assert_eq!(backoff(10), Duration::from_secs(30));
    }

    #[test]
    fn build_path_public_no_filter() {
        assert_eq!(build_path(None, None, false), "/v1/events");
    }

    #[test]
    fn build_path_public_with_markets() {
        let path = build_path(Some("abc,def"), None, false);
        assert_eq!(path, "/v1/events?markets=abc,def");
    }

    #[test]
    fn build_path_private_no_filter() {
        assert_eq!(build_path(None, None, true), "/v1/events/private");
    }

    #[test]
    fn build_path_private_with_markets() {
        let path = build_path(Some("u1"), None, true);
        assert_eq!(path, "/v1/events/private?markets=u1");
    }

    #[test]
    fn build_path_with_types_only() {
        let path = build_path(None, Some("amm.trade,amm.price_update"), false);
        assert_eq!(path, "/v1/events?types=amm.trade,amm.price_update");
    }

    #[test]
    fn build_path_markets_and_types_combined() {
        let path = build_path(Some("u1,u2"), Some("amm.trade"), true);
        assert_eq!(path, "/v1/events/private?markets=u1,u2&types=amm.trade");
    }

    #[test]
    fn follow_set_tracks_lifecycle() {
        let mut set: HashSet<String> = HashSet::new();
        let env = json!({"type":"market.opened","market_id":"m1","data":{}});
        update_follow_set(&mut set, "market.opened", &env);
        assert!(set.contains("m1"));

        let env2 = json!({"type":"market.voided","market_id":"m1","data":{}});
        update_follow_set(&mut set, "market.voided", &env2);
        assert!(!set.contains("m1"));
    }

    // ── D6: compute_server_types lifecycle injection ───────────────

    #[test]
    fn compute_server_types_no_filter_no_follow_open() {
        assert_eq!(compute_server_types(None, false), None);
    }

    #[test]
    fn compute_server_types_no_filter_follow_open() {
        // No user filter: server streams everything, lifecycle events
        // arrive naturally, no injection needed.
        assert_eq!(compute_server_types(None, true), None);
    }

    #[test]
    fn compute_server_types_user_filter_no_follow_open() {
        assert_eq!(
            compute_server_types(Some("amm.trade"), false),
            Some("amm.trade".to_string())
        );
    }

    #[test]
    fn compute_server_types_follow_open_injects_lifecycle() {
        // The user asked for amm.trade; under --follow-open we must also
        // request market.opened, market.resolved, market.voided so the
        // client can maintain its follow set.
        let got = compute_server_types(Some("amm.trade"), true).unwrap();
        for lt in ["amm.trade", "market.opened", "market.resolved", "market.voided"] {
            assert!(got.contains(lt), "expected '{lt}' in server types, got: {got}");
        }
    }

    #[test]
    fn compute_server_types_follow_open_does_not_double_inject() {
        // User already asked for market.opened explicitly; we should not
        // inject it a second time. The other lifecycle types must still
        // be added.
        let got = compute_server_types(Some("market.opened,amm.trade"), true).unwrap();
        let parts: Vec<&str> = got.split(',').collect();
        let opened_count = parts.iter().filter(|p| **p == "market.opened").count();
        assert_eq!(opened_count, 1, "market.opened must appear exactly once");
        assert!(parts.contains(&"market.resolved"));
        assert!(parts.contains(&"market.voided"));
        assert!(parts.contains(&"amm.trade"));
    }

    #[test]
    fn compute_server_types_follow_open_with_all_lifecycle_is_noop() {
        let got = compute_server_types(
            Some("market.opened,market.resolved,market.voided"),
            true,
        )
        .unwrap();
        // All three present; each exactly once.
        for lt in ["market.opened", "market.resolved", "market.voided"] {
            assert_eq!(
                got.matches(lt).count(),
                1,
                "{lt} must appear exactly once in: {got}"
            );
        }
    }
}
