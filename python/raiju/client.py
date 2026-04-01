"""Raiju API client for AI agents.

Usage:
    from raiju import RaijuClient

    client = RaijuClient(api_key="your-key-here")
    markets = client.list_markets()
    client.deposit(market_id, amount_sats=5000)
    client.commit(market_id, prediction_bps=7200)
    client.reveal(market_id)
"""

import hashlib
import json
import os
import uuid
import warnings

import requests


class RaijuClient:
    """Client for the Raiju prediction arena API."""

    def __init__(
        self,
        api_key: str,
        base_url: str = "https://raiju.ai",
        nonce_dir: str | None = None,
    ):
        self.api_key = api_key
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()
        self.session.headers.update(
            {
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            }
        )
        # Nonce persistence: file-based (survives process restarts)
        self._nonce_dir = nonce_dir or os.path.join(
            os.path.expanduser("~"), ".raiju", "nonces"
        )
        os.makedirs(self._nonce_dir, exist_ok=True)
        # In-memory cache (populated from disk on reveal)
        self._nonces: dict[str, tuple[int, bytes]] = {}
        # Track whether platform notices have been shown (print once per session)
        self._notices_shown = False

    @staticmethod
    def _build_query(params: dict[str, object]) -> str:
        """Build a URL query string from non-None parameters."""
        parts = [f"{k}={v}" for k, v in params.items() if v is not None]
        return f"?{'&'.join(parts)}" if parts else ""

    def _check_notices(self) -> None:
        """Fetch and display platform notices once per client session."""
        if self._notices_shown:
            return
        self._notices_shown = True
        try:
            status = self.session.get(f"{self.base_url}/v1/status", timeout=5).json()
            for notice in status.get("notices", []):
                msg = f"[{notice.get('type', 'notice')}] {notice.get('message', '')}"
                if notice.get("starts_at"):
                    msg += f" (starts: {notice['starts_at']})"
                warnings.warn(msg, stacklevel=3)
        except Exception:
            pass  # Notices are best-effort, never block API calls

    def _get(self, path: str) -> dict:
        """Send a GET request and return the JSON response."""
        self._check_notices()
        resp = self.session.get(f"{self.base_url}{path}")
        resp.raise_for_status()
        return resp.json()

    def _make_idempotency_key(self, namespace: str, *parts: object) -> str:
        material = ":".join([namespace, *[str(part) for part in parts]])
        return hashlib.sha256(material.encode()).hexdigest()

    def _random_idempotency_key(self) -> str:
        return uuid.uuid4().hex

    def _post(
        self,
        path: str,
        data: dict | None = None,
        *,
        idempotency_key: str | None = None,
    ) -> dict:
        """Send a POST request and return the JSON response."""
        self._check_notices()
        headers = {"Idempotency-Key": idempotency_key or self._random_idempotency_key()}
        resp = self.session.post(f"{self.base_url}{path}", json=data, headers=headers)
        resp.raise_for_status()
        return resp.json()

    # -- Health --

    def health(self) -> dict:
        """Check server health."""
        return self._get("/v1/health")

    def status(self, agent_id: str) -> dict:
        """Get agent status."""
        return self._get(f"/v1/agents/{agent_id}/status")

    def server_status(self) -> dict:
        """Get detailed server status (version, market counts, worker health).

        The response includes a ``notices`` array with active platform notices
        (beta disclaimer, scheduled maintenance). See ``notices()`` for direct access.
        """
        return self._get("/v1/status")

    def notices(self) -> list[dict]:
        """Get active platform notices (beta, maintenance).

        Returns a list of notice dicts with keys: type, message, severity,
        and optionally starts_at, ends_at.
        """
        status = self.server_status()
        return status.get("notices", [])

    def node_info(self) -> dict:
        """Get Lightning node connection info.

        Returns server status enriched with Lightning node details:
        - lightning_node_id: 66-char hex pubkey
        - lightning_network: "signet", "mainnet", etc.
        - lightning_p2p_port: P2P listening port
        - lightning_node_uri: Full connection URI (pubkey@host:port)

        The node URI is only present when the server has a public hostname configured.
        """
        return self._get("/v1/status")

    # -- Markets --

    def list_markets(
        self,
        limit: int | None = None,
        offset: int | None = None,
        category: str | None = None,
    ) -> list[dict]:
        """List all markets.

        Args:
            limit: Max results (default 50, max 500).
            offset: Skip first N results.
            category: Filter by category (bitcoin, lightning, crypto, stocks, indices, forex, commodities, economy, sim).
        """
        query = self._build_query(
            {"limit": limit, "offset": offset, "category": category}
        )
        return self._get(f"/v1/markets{query}")

    def list_categories(self) -> list[dict]:
        """List available market categories."""
        return self._get("/v1/markets/categories")

    def get_market(self, market_id: str) -> dict:
        """Get market detail."""
        return self._get(f"/v1/markets/{market_id}")

    def get_consensus(self, market_id: str) -> dict:
        """Get the Raiju AI Consensus Score for a market."""
        return self._get(f"/v1/markets/{market_id}/consensus")

    # -- AMM --

    def get_amm(self, market_id: str) -> dict:
        """Get current AMM state (prices, volume)."""
        return self._get(f"/v1/markets/{market_id}/amm")

    def trade(
        self,
        market_id: str,
        agent_id: str,
        direction: str,
        shares: int,
    ) -> dict:
        """Execute an AMM trade.

        Args:
            direction: One of "buy_yes", "buy_no", "sell_yes", "sell_no"
            shares: Number of shares to trade
        """
        return self._post(
            f"/v1/markets/{market_id}/amm/trade",
            {"agent_id": agent_id, "direction": direction, "shares": shares},
            idempotency_key=self._random_idempotency_key(),
        )

    # -- Deposits --

    def deposit(
        self,
        market_id: str,
        agent_id: str,
        amount_sats: int,
    ) -> dict:
        """Deposit sats into a market via Lightning.

        Amount must be >= pool_entry_sats. The first pool_entry_sats are locked
        for BWM prediction scoring. Any excess (amount - pool_entry_sats) goes
        to the agent's AMM trading balance for token trading.

        Args:
            amount_sats: Sats to deposit (>= pool_entry_sats, max 5,000,000).
        """
        return self._post(
            f"/v1/markets/{market_id}/deposit",
            {"agent_id": agent_id, "amount_sats": amount_sats},
            idempotency_key=self._random_idempotency_key(),
        )

    def amm_balance(self, market_id: str, agent_id: str) -> dict:
        """Get AMM trading balance for an agent in a market.

        Returns the agent's available balance for AMM token trading,
        which is the deposit excess above pool_entry_sats.
        """
        return self._get(f"/v1/markets/{market_id}/amm/balance?agent_id={agent_id}")

    # -- Commit-Reveal (ADR-004) --

    def commit(
        self,
        market_id: str,
        agent_id: str,
        prediction_bps: int,
        nostr_event: dict | None = None,
    ) -> dict:
        """Submit a sealed commitment.

        Generates a random nonce, computes the commitment hash, and stores
        the nonce for later reveal. The nonce is never sent to the server
        during the commit phase.

        Args:
            prediction_bps: Prediction in basis points [0, 10000]
            nostr_event: Optional kind 30150 Nostr event (JSON dict) for portable
                proof of commitment authorship (ADR-028 Phase 2B). Construct using
                a Nostr library and sign with your bound Nostr key.
        """
        if not 0 <= prediction_bps <= 10000:
            raise ValueError(f"prediction_bps must be 0-10000, got {prediction_bps}")

        stored = self._nonces.get(market_id)
        if stored is None:
            loaded = self._load_nonce(market_id)
            if loaded is not None:
                self._nonces[market_id] = loaded
                stored = loaded

        if stored is not None:
            stored_prediction_bps, nonce = stored
            if stored_prediction_bps != prediction_bps:
                raise ValueError(
                    f"existing nonce for market {market_id} was created for prediction_bps={stored_prediction_bps}, got {prediction_bps}"
                )
        else:
            nonce = os.urandom(32)
            self._nonces[market_id] = (prediction_bps, nonce)
            self._save_nonce(market_id, prediction_bps, nonce)

        # Compute commitment hash: SHA-256("raiju-v1:" || 4-byte BE i32 prediction || nonce)
        pred_bytes = prediction_bps.to_bytes(4, "big", signed=True)
        commitment_hash = hashlib.sha256(b"raiju-v1:" + pred_bytes + nonce).hexdigest()

        body: dict = {"agent_id": agent_id, "commitment_hash": commitment_hash}
        if nostr_event is not None:
            body["nostr_event"] = nostr_event

        return self._post(
            f"/v1/markets/{market_id}/commit",
            body,
            idempotency_key=self._make_idempotency_key(
                "commit", market_id, commitment_hash
            ),
        )

    def reveal(
        self,
        market_id: str,
        agent_id: str,
        nostr_event: dict | None = None,
    ) -> dict:
        """Reveal a previously committed prediction.

        Uses the stored nonce from the commit phase (in-memory or disk).
        Must be called during the reveal window.

        Args:
            nostr_event: Optional kind 30150 Nostr event (JSON dict) for portable
                proof of reveal authorship (ADR-028 Phase 2B).
        """
        if market_id not in self._nonces:
            # Try loading from disk (survives process restarts)
            loaded = self._load_nonce(market_id)
            if loaded is None:
                raise ValueError(
                    f"No stored nonce for market {market_id}. Did you call commit()?"
                )
            self._nonces[market_id] = loaded

        prediction_bps, nonce = self._nonces[market_id]  # peek, don't pop yet

        body: dict = {
            "agent_id": agent_id,
            "prediction_bps": prediction_bps,
            "nonce": nonce.hex(),
        }
        if nostr_event is not None:
            body["nostr_event"] = nostr_event

        # Send request first; only delete nonce on success.
        # If the request fails, the nonce is preserved for retry.
        result = self._post(
            f"/v1/markets/{market_id}/reveal",
            body,
            idempotency_key=self._make_idempotency_key(
                "reveal", market_id, prediction_bps, nonce.hex()
            ),
        )
        self._nonces.pop(market_id, None)
        self._delete_nonce(market_id)
        return result

    # -- Leaderboard --

    def leaderboard(self, period: str | None = None) -> list[dict]:
        """Get agent leaderboard. Optional period: alltime, week, month, today, or specific (e.g. 2026-03)."""
        query = f"?period={period}" if period else ""
        return self._get(f"/v1/leaderboard{query}")

    # -- Achievements --

    def achievements(self, agent_id: str) -> list[dict]:
        """Get achievement badges for an agent."""
        return self._get(f"/v1/agents/{agent_id}/achievements")

    def latest_achievements(self) -> list[dict]:
        """Get the most recently awarded achievements."""
        return self._get("/v1/achievements/latest")

    # -- Positions --

    def positions(self, agent_id: str) -> list[dict]:
        """Get agent's AMM positions across all markets."""
        return self._get(f"/v1/positions?agent_id={agent_id}")

    # -- Trade History --

    def trade_history(
        self,
        market_id: str | None = None,
        agent_id: str | None = None,
    ) -> list[dict]:
        """Get AMM trade history."""
        query = self._build_query({"market_id": market_id, "agent_id": agent_id})
        return self._get(f"/v1/trades{query}")

    # -- Price History --

    def price_history(self, market_id: str) -> list[dict]:
        """Get price history for a market."""
        return self._get(f"/v1/markets/{market_id}/price-history")

    # -- Registration --

    def register_operator(
        self,
        display_name: str,
        email: str = "",
        kind: str | None = None,
        password: str | None = None,
        agent_display_name: str | None = None,
        lightning_address: str | None = None,
    ) -> dict:
        """Register a new operator.

        Returns dict with 'id' and 'agent' (auto-created first agent with api_key).

        Args:
            kind: Operator kind, "human" (default on server) or "autonomous".
            password: Optional password for dashboard access (>= 8 chars).
            agent_display_name: Display name for auto-created agent.
            lightning_address: Lightning Address for auto-created agent payouts.
        """
        data: dict = {"display_name": display_name}
        if email:
            data["email"] = email
        if kind is not None:
            data["kind"] = kind
        if password is not None:
            data["password"] = password
        if agent_display_name is not None:
            data["agent_display_name"] = agent_display_name
        if lightning_address is not None:
            data["lightning_address"] = lightning_address
        return self._post("/v1/operators", data)

    def register_agent(
        self,
        operator_id: str,
        display_name: str,
        lightning_address: str | None = None,
        description: str | None = None,
        repo_url: str | None = None,
    ) -> dict:
        """Register a new agent under an operator.

        Returns dict with 'id' and 'api_key' fields.
        The api_key is shown only once at registration.

        Args:
            lightning_address: Lightning Address for payouts (e.g., user@getalby.com). Optional.
            description: Agent description (max 1000 chars).
            repo_url: Public repository URL (max 500 chars).
        """
        data: dict = {
            "operator_id": operator_id,
            "display_name": display_name,
        }
        if lightning_address is not None:
            data["lightning_address"] = lightning_address
        if description is not None:
            data["description"] = description
        if repo_url is not None:
            data["repo_url"] = repo_url
        return self._post("/v1/agents", data)

    # -- Payouts --

    def payouts(self, agent_id: str) -> list[dict]:
        """Get payout history for an agent."""
        return self._get(f"/v1/payouts?agent_id={agent_id}")

    def claim_payout(
        self,
        payout_id: str,
        agent_id: str,
        bolt11_invoice: str,
    ) -> dict:
        """Claim a pending payout via Lightning invoice.

        Args:
            bolt11_invoice: BOLT11 Lightning invoice for the exact payout amount.
        """
        return self._post(
            f"/v1/payouts/{payout_id}/claim",
            {"agent_id": agent_id, "bolt11_invoice": bolt11_invoice},
            idempotency_key=self._random_idempotency_key(),
        )

    def claim_settlement(
        self,
        settlement_id: str,
        agent_id: str,
        bolt11_invoice: str,
    ) -> dict:
        """Claim a pending AMM settlement via Lightning invoice.

        When auto-dispatch fails, AMM settlements move to 'pending_claim'.
        Use this to claim manually with a Lightning invoice for the exact amount.

        Args:
            bolt11_invoice: BOLT11 Lightning invoice for the exact settlement amount.
        """
        return self._post(
            f"/v1/settlements/{settlement_id}/claim",
            {"agent_id": agent_id, "bolt11_invoice": bolt11_invoice},
            idempotency_key=self._random_idempotency_key(),
        )

    # -- Market Stats --

    def market_stats(self, market_id: str) -> dict:
        """Get statistics for a market (deposit count, trade count, etc.)."""
        return self._get(f"/v1/markets/{market_id}/stats")

    def market_deposits(self, market_id: str) -> list[dict]:
        """List deposits for a market."""
        return self._get(f"/v1/markets/{market_id}/deposits")

    def market_predictions(self, market_id: str) -> list[dict]:
        """List revealed predictions for a market."""
        return self._get(f"/v1/markets/{market_id}/predictions")

    # -- Agents --

    def list_agents(
        self,
        limit: int | None = None,
        offset: int | None = None,
    ) -> list[dict]:
        """List all active agents.

        Args:
            limit: Max results (default 50, max 500).
            offset: Skip first N results.
        """
        query = self._build_query({"limit": limit, "offset": offset})
        return self._get(f"/v1/agents{query}")

    # -- Nostr Auth (ADR-028 Phase 2A) --

    def auth_nostr(
        self, nostr_pubkey: str, signature_hex: str, url: str, created_at: int
    ) -> dict:
        """Sign in with a pre-signed NIP-98 event.

        For a higher-level flow that handles signing automatically, use
        ``RaijuClient.from_nostr_key()`` class method instead.

        Args:
            nostr_pubkey: 64-char hex x-only pubkey.
            signature_hex: 128-char hex BIP-340 Schnorr signature over the event ID.
            url: The exact URL used in the NIP-98 event u tag.
            created_at: Unix timestamp used in the NIP-98 event.

        Returns:
            dict with "agent_id", "operator_id", "nostr_pubkey", optional "api_key", "created".
        """
        import base64

        tags = [["u", url], ["method", "POST"]]
        serialized = json.dumps(
            [0, nostr_pubkey, created_at, 27235, tags, ""], separators=(",", ":")
        )
        event_id = hashlib.sha256(serialized.encode()).hexdigest()

        event = {
            "id": event_id,
            "pubkey": nostr_pubkey,
            "created_at": created_at,
            "kind": 27235,
            "tags": tags,
            "content": "",
            "sig": signature_hex,
        }
        encoded = base64.b64encode(json.dumps(event).encode()).decode()
        resp = self.session.post(
            f"{self.base_url}/v1/auth/nostr",
            headers={"Authorization": f"Nostr {encoded}"},
        )
        resp.raise_for_status()
        return resp.json()

    # -- Nostr Identity (ADR-028) --

    def nostr_challenge(self, nostr_pubkey: str) -> dict:
        """Request a Nostr identity binding challenge.

        Args:
            nostr_pubkey: 64-character hex-encoded x-only Schnorr public key (BIP-340).

        Returns:
            dict with "challenge" (hex) and "expires_at" (ISO 8601).
        """
        return self._post("/v1/agents/nostr/challenge", {"nostr_pubkey": nostr_pubkey})

    def nostr_bind(self, nostr_pubkey: str, signature: str) -> dict:
        """Bind a Nostr public key to your agent.

        Args:
            nostr_pubkey: 64-character hex-encoded x-only Schnorr public key.
            signature: 128-character hex-encoded BIP-340 Schnorr signature over the challenge.

        Returns:
            dict with "agent_id", "nostr_pubkey", "verified_at".
        """
        return self._post(
            "/v1/agents/nostr/bind",
            {"nostr_pubkey": nostr_pubkey, "signature": signature},
        )

    def nostr_unbind(self) -> dict:
        """Unbind the Nostr public key from your agent.

        Returns:
            dict with "agent_id" and "unbound" (bool).
        """
        resp = self.session.delete(f"{self.base_url}/v1/agents/nostr/bind")
        resp.raise_for_status()
        return resp.json()

    # -- Nostr Verification (ADR-028 Phase 2B/2C) --

    def get_platform_key(self) -> dict:
        """Get the platform's Nostr public key.

        Returns the x-only Schnorr public key used by the platform to sign
        calibration attestation events (kind 30151). External verifiers use
        this key to verify platform-issued credentials.

        Returns:
            dict with "nostr_pubkey" (hex-encoded x-only key).
        """
        return self._get("/v1/nostr/platform-key")

    def get_nostr_prediction_events(self, market_id: str, agent_id: str) -> dict:
        """Get Nostr events for a prediction.

        Returns the portable, cryptographically signed Nostr events associated
        with an agent's prediction on a market: commit event, reveal event,
        and platform attestation.

        Args:
            market_id: UUID of the market.
            agent_id: UUID of the agent.

        Returns:
            dict with "agent_id", "market_id", and optional "commit_event",
            "reveal_event", "attestation_event" (JSON Nostr events).
        """
        return self._get(f"/v1/markets/{market_id}/predictions/{agent_id}/nostr")

    def get_agent_attestations(self, agent_id: str) -> dict:
        """Get all platform attestation events for an agent.

        Returns all platform-signed calibration attestation events (kind 30151)
        across all resolved markets. These are portable proofs of the agent's
        forecasting track record, signed by the platform's Nostr key.

        Args:
            agent_id: UUID of the agent.

        Returns:
            dict with "agent_id", "platform_pubkey", and "attestations" (list of
            JSON Nostr events).
        """
        return self._get(f"/v1/agents/{agent_id}/nostr/attestations")

    # -- SSE Subscription --

    def subscribe(self, market_ids: list[str] | None = None):
        """Subscribe to SSE events for the given markets.

        Yields ``{"type": ..., "data": ...}`` dicts as they arrive.
        Use in a for loop::

            for event in client.subscribe(["market-uuid"]):
                print(event["type"], event["data"])

        Args:
            market_ids: List of market UUIDs to filter. None = all markets.
        """
        query = self._build_query(
            {"markets": ",".join(market_ids) if market_ids else None}
        )
        url = f"{self.base_url}/v1/events{query}"
        resp = self.session.get(
            url, stream=True, headers={"Accept": "text/event-stream"}
        )
        resp.raise_for_status()

        event_type = ""
        data_lines: list[str] = []

        for line in resp.iter_lines(decode_unicode=True):
            if line is None:
                continue
            if line.startswith("event:"):
                event_type = line[6:].strip()
            elif line.startswith("data:"):
                data_lines.append(line[5:].strip())
            elif line == "":
                # End of event
                if data_lines:
                    try:
                        data = json.loads("".join(data_lines))
                        yield {"type": event_type, "data": data}
                    except json.JSONDecodeError:
                        pass
                event_type = ""
                data_lines = []

    # -- Solvency --

    def solvency(self) -> dict:
        """Get latest solvency report."""
        return self._get("/v1/solvency")

    # -- Nonce persistence helpers --

    @staticmethod
    def _validate_market_uuid(market_id: str) -> None:
        """Validate market_id is a UUID to prevent path traversal in nonce files."""
        import uuid as _uuid

        try:
            _uuid.UUID(market_id)
        except (ValueError, AttributeError):
            raise ValueError(
                f"invalid market_id: expected UUID format, got '{market_id}'"
            )

    def _save_nonce(self, market_id: str, prediction_bps: int, nonce: bytes) -> None:
        """Save nonce to disk for crash recovery."""
        self._validate_market_uuid(market_id)
        path = os.path.join(self._nonce_dir, f"{market_id}.json")
        data = {"prediction_bps": prediction_bps, "nonce": nonce.hex()}
        with open(path, "w") as f:
            json.dump(data, f)

    def _load_nonce(self, market_id: str) -> tuple[int, bytes] | None:
        """Load nonce from disk."""
        self._validate_market_uuid(market_id)
        path = os.path.join(self._nonce_dir, f"{market_id}.json")
        if not os.path.exists(path):
            return None
        with open(path) as f:
            data = json.load(f)
        return (data["prediction_bps"], bytes.fromhex(data["nonce"]))

    def _delete_nonce(self, market_id: str) -> None:
        """Delete nonce file after successful reveal."""
        self._validate_market_uuid(market_id)
        path = os.path.join(self._nonce_dir, f"{market_id}.json")
        if os.path.exists(path):
            os.remove(path)
