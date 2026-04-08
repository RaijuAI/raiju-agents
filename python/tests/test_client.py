"""Tests for the Raiju Python client.

These tests verify the client's internal logic without requiring a running server.
"""

import hashlib
import json
import os
from unittest.mock import MagicMock, patch

import pytest

from raiju.client import RaijuClient


def test_client_default_url():
    """Verify default API URL."""
    client = RaijuClient(api_key="test")
    assert client.base_url == "https://raiju.ai"


def test_client_custom_url():
    """Verify custom API URL."""
    client = RaijuClient(api_key="test", base_url="https://api.raiju.ai")
    assert client.base_url == "https://api.raiju.ai"


def test_client_url_trailing_slash():
    """Verify trailing slash is stripped."""
    client = RaijuClient(api_key="test", base_url="http://example.com/")
    assert client.base_url == "http://example.com"


def test_auth_header_set():
    """Verify Authorization header is set correctly."""
    client = RaijuClient(api_key="abc123")
    assert client.session.headers["Authorization"] == "Bearer abc123"


def test_content_type_header():
    """Verify Content-Type header is set to JSON."""
    client = RaijuClient(api_key="test")
    assert client.session.headers["Content-Type"] == "application/json"


def test_prediction_bps_validation_high():
    """Verify commit rejects prediction_bps > 10000."""
    client = RaijuClient(api_key="test")
    with pytest.raises(ValueError, match="0-10000"):
        client.commit("market", "agent", prediction_bps=10001)


def test_prediction_bps_validation_negative():
    """Verify commit rejects prediction_bps < 0."""
    client = RaijuClient(api_key="test")
    with pytest.raises(ValueError, match="0-10000"):
        client.commit("market", "agent", prediction_bps=-1)


def test_reveal_without_commit():
    """Verify reveal fails without a stored nonce."""
    client = RaijuClient(api_key="test")
    with pytest.raises(ValueError, match="No stored nonce"):
        client.reveal(
            "550e8400-e29b-41d4-a716-446655440099",
            "11111111-1111-1111-1111-111111111111",
        )


def test_nonce_storage_per_market():
    """Verify nonces are stored separately per (agent, market) key."""
    client = RaijuClient(api_key="test")
    client._nonces[("agent-1", "m1")] = (5000, os.urandom(32))
    client._nonces[("agent-1", "m2")] = (7000, os.urandom(32))
    assert len(client._nonces) == 2
    assert client._nonces[("agent-1", "m1")][0] == 5000
    assert client._nonces[("agent-1", "m2")][0] == 7000


def test_nonce_cleared_after_pop():
    """Verify nonce is removed from cache after pop (simulating reveal)."""
    client = RaijuClient(api_key="test")
    key = ("agent-1", "m1")
    client._nonces[key] = (5000, os.urandom(32))
    pred, nonce = client._nonces.pop(key)
    assert pred == 5000
    assert key not in client._nonces


def test_commitment_hash_deterministic():
    """Verify the same inputs always produce the same commitment hash."""
    prediction_bps = 7200
    nonce = bytes.fromhex("aa" * 32)
    pred_bytes = prediction_bps.to_bytes(4, "big", signed=True)
    h1 = hashlib.sha256(b"raiju-v1:" + pred_bytes + nonce).hexdigest()
    h2 = hashlib.sha256(b"raiju-v1:" + pred_bytes + nonce).hexdigest()
    assert h1 == h2
    assert len(h1) == 64
    assert all(c in "0123456789abcdef" for c in h1)


def test_build_query_all_none():
    """Verify _build_query returns empty string when all params are None."""
    assert RaijuClient._build_query({"a": None, "b": None}) == ""


def test_build_query_mixed():
    """Verify _build_query includes only non-None params."""
    result = RaijuClient._build_query(
        {"limit": 10, "offset": None, "category": "bitcoin"}
    )
    assert "limit=10" in result
    assert "category=bitcoin" in result
    assert "offset" not in result
    assert result.startswith("?")


def test_claim_payout_requires_bolt11():
    """Verify claim_payout requires bolt11_invoice (Lightning-only)."""
    client = RaijuClient(api_key="test")
    with pytest.raises(TypeError):
        client.claim_payout("p1", "a1")  # missing required bolt11_invoice


def test_nonce_disk_persistence(tmp_path):
    """Verify nonce save/load/delete round-trip on disk."""
    client = RaijuClient(api_key="test", nonce_dir=str(tmp_path))
    nonce = os.urandom(32)
    agent_id = "11111111-1111-1111-1111-111111111111"
    market_id = "550e8400-e29b-41d4-a716-446655440001"
    client._save_nonce(agent_id, market_id, 7200, nonce)

    loaded = client._load_nonce(agent_id, market_id)
    assert loaded is not None
    assert loaded[0] == 7200
    assert loaded[1] == nonce

    client._delete_nonce(agent_id, market_id)
    assert client._load_nonce(agent_id, market_id) is None


def test_audit_commit_retry_preserves_original_nonce_after_ambiguous_failure(tmp_path):
    """A retry after an ambiguous commit failure must not overwrite the original nonce."""
    client = RaijuClient(api_key="test", nonce_dir=str(tmp_path))
    market_id = "550e8400-e29b-41d4-a716-446655440099"
    agent_id = "11111111-1111-1111-1111-111111111111"
    nonce1 = bytes.fromhex("11" * 32)
    nonce2 = bytes.fromhex("22" * 32)

    with patch("os.urandom", side_effect=[nonce1, nonce2]):
        with patch.object(
            client,
            "_post",
            side_effect=[
                RuntimeError("timeout after server accepted commit"),
                RuntimeError("already committed"),
            ],
        ):
            with pytest.raises(RuntimeError):
                client.commit(market_id, agent_id, 5000)

            stored = client._load_nonce(agent_id, market_id)
            assert stored is not None
            assert stored[1] == nonce1

            with pytest.raises(RuntimeError):
                client.commit(market_id, agent_id, 5000)

    loaded = client._load_nonce(agent_id, market_id)
    assert loaded is not None
    assert loaded[1] == nonce1, "retry must preserve the original reveal nonce"


def test_audit_protected_write_includes_idempotency_key():
    """Protected writes should include an Idempotency-Key so retries are safe."""
    client = RaijuClient(api_key="test")
    with patch.object(
        client.session, "post", return_value=_mock_response({"id": "d1"})
    ) as mock_post:
        client.deposit("market-1", "agent-1", 10000)

    headers = mock_post.call_args.kwargs.get("headers") or {}
    assert "Idempotency-Key" in headers, "protected writes must send an Idempotency-Key"


# ---------------------------------------------------------------------------
# Helper to mock HTTP responses
# ---------------------------------------------------------------------------


def _mock_response(json_data, status_code=200):
    """Create a mock requests.Response with the given JSON data."""
    mock = MagicMock()
    mock.status_code = status_code
    mock.json.return_value = json_data
    mock.raise_for_status.return_value = None
    return mock


# ---------------------------------------------------------------------------
# Tests for list_categories
# ---------------------------------------------------------------------------


class TestListCategories:
    """Tests for the list_categories method."""

    def test_list_categories_calls_correct_endpoint(self):
        """Verify list_categories hits /v1/markets/categories."""
        client = RaijuClient(api_key="test")
        expected = [{"slug": "bitcoin", "name": "Bitcoin", "market_count": 5}]
        with patch.object(
            client.session, "get", return_value=_mock_response(expected)
        ) as mock_get:
            result = client.list_categories()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/markets/categories")
            assert result == expected

    def test_list_categories_returns_empty_list(self):
        """Verify list_categories handles empty response."""
        client = RaijuClient(api_key="test")
        with patch.object(client.session, "get", return_value=_mock_response([])):
            result = client.list_categories()
            assert result == []


# ---------------------------------------------------------------------------
# Tests for Nostr identity methods (ADR-028)
# ---------------------------------------------------------------------------


class TestNostrChallenge:
    """Tests for the nostr_challenge method."""

    def test_nostr_challenge_sends_correct_payload(self):
        """Verify nostr_challenge posts pubkey to /v1/agents/nostr/challenge."""
        client = RaijuClient(api_key="test")
        pubkey = "a" * 64
        expected = {"challenge": "deadbeef" * 8, "expires_at": "2026-03-28T00:00:00Z"}
        with patch.object(
            client.session, "post", return_value=_mock_response(expected)
        ) as mock_post:
            result = client.nostr_challenge(pubkey)
            call_url = mock_post.call_args[0][0]
            call_json = mock_post.call_args[1]["json"]
            assert call_url.endswith("/v1/agents/nostr/challenge")
            assert call_json == {"nostr_pubkey": pubkey}
            assert result["challenge"] == "deadbeef" * 8

    def test_nostr_challenge_preserves_hex_pubkey(self):
        """Verify the pubkey is sent as-is without transformation."""
        client = RaijuClient(api_key="test")
        pubkey = "0123456789abcdef" * 4
        with patch.object(
            client.session,
            "post",
            return_value=_mock_response({"challenge": "ff" * 32}),
        ) as mock_post:
            client.nostr_challenge(pubkey)
            call_data = mock_post.call_args[1]["json"]
            assert call_data["nostr_pubkey"] == pubkey


class TestNostrBind:
    """Tests for the nostr_bind method."""

    def test_nostr_bind_sends_correct_payload(self):
        """Verify nostr_bind posts pubkey and signature to /v1/agents/nostr/bind."""
        client = RaijuClient(api_key="test")
        pubkey = "a" * 64
        sig = "b" * 128
        expected = {
            "agent_id": "uuid-1",
            "nostr_pubkey": pubkey,
            "verified_at": "2026-03-28T00:00:00Z",
        }
        with patch.object(
            client.session, "post", return_value=_mock_response(expected)
        ) as mock_post:
            result = client.nostr_bind(pubkey, sig)
            call_url = mock_post.call_args[0][0]
            call_json = mock_post.call_args[1]["json"]
            assert call_url.endswith("/v1/agents/nostr/bind")
            assert call_json == {"nostr_pubkey": pubkey, "signature": sig}
            assert result["nostr_pubkey"] == pubkey


class TestNostrUnbind:
    """Tests for the nostr_unbind method."""

    def test_nostr_unbind_calls_delete(self):
        """Verify nostr_unbind sends DELETE to /v1/agents/nostr/bind."""
        client = RaijuClient(api_key="test")
        expected = {"agent_id": "uuid-1", "unbound": True}
        with patch.object(
            client.session, "delete", return_value=_mock_response(expected)
        ) as mock_del:
            result = client.nostr_unbind()
            call_url = mock_del.call_args[0][0]
            assert call_url.endswith("/v1/agents/nostr/bind")
            assert result["unbound"] is True


class TestAuthNostr:
    """Tests for the auth_nostr method."""

    def test_auth_nostr_constructs_nip98_event(self):
        """Verify auth_nostr builds correct NIP-98 event and sends base64 header."""
        import base64

        client = RaijuClient(api_key="test")
        pubkey = "a" * 64
        sig = "b" * 128
        url = "http://localhost:3001/v1/auth/nostr"
        created_at = 1711584000

        expected_resp = {
            "agent_id": "uuid-1",
            "operator_id": "op-1",
            "nostr_pubkey": pubkey,
        }
        with patch.object(
            client.session, "post", return_value=_mock_response(expected_resp)
        ) as mock_post:
            result = client.auth_nostr(pubkey, sig, url, created_at)

            # Verify the Authorization header contains a base64-encoded Nostr event
            call_kwargs = mock_post.call_args
            auth_header = call_kwargs[1]["headers"]["Authorization"]
            assert auth_header.startswith("Nostr ")
            decoded = json.loads(base64.b64decode(auth_header[6:]))
            assert decoded["pubkey"] == pubkey
            assert decoded["sig"] == sig
            assert decoded["kind"] == 27235
            assert decoded["created_at"] == created_at
            assert ["u", url] in decoded["tags"]
            assert ["method", "POST"] in decoded["tags"]
            assert result["agent_id"] == "uuid-1"

    def test_auth_nostr_event_id_is_sha256(self):
        """Verify the event ID is SHA-256 of the canonical serialized event."""
        client = RaijuClient(api_key="test")
        pubkey = "c" * 64
        sig = "d" * 128
        url = "http://localhost:3001/v1/auth/nostr"
        created_at = 1711584000

        with patch.object(
            client.session, "post", return_value=_mock_response({})
        ) as mock_post:
            client.auth_nostr(pubkey, sig, url, created_at)
            import base64

            auth_header = mock_post.call_args[1]["headers"]["Authorization"]
            decoded = json.loads(base64.b64decode(auth_header[6:]))

            # Recompute expected event ID
            tags = [["u", url], ["method", "POST"]]
            serialized = json.dumps(
                [0, pubkey, created_at, 27235, tags, ""], separators=(",", ":")
            )
            expected_id = hashlib.sha256(serialized.encode()).hexdigest()
            assert decoded["id"] == expected_id


# ---------------------------------------------------------------------------
# Tests for Nostr verification methods (ADR-028 Phase 2B/2C)
# ---------------------------------------------------------------------------


class TestGetPlatformKey:
    """Tests for the get_platform_key method."""

    def test_get_platform_key_calls_correct_endpoint(self):
        """Verify get_platform_key hits /v1/nostr/platform-key."""
        client = RaijuClient(api_key="test")
        expected = {"nostr_pubkey": "e" * 64}
        with patch.object(
            client.session, "get", return_value=_mock_response(expected)
        ) as mock_get:
            result = client.get_platform_key()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/nostr/platform-key")
            assert result["nostr_pubkey"] == "e" * 64


class TestGetNostrPredictionEvents:
    """Tests for the get_nostr_prediction_events method."""

    def test_get_nostr_prediction_events_correct_url(self):
        """Verify URL path uses market_id and agent_id."""
        client = RaijuClient(api_key="test")
        market_id = "m-uuid-1"
        agent_id = "a-uuid-1"
        expected = {
            "agent_id": agent_id,
            "market_id": market_id,
            "commit_event": {"kind": 30150},
            "reveal_event": None,
            "attestation_event": None,
        }
        with patch.object(
            client.session, "get", return_value=_mock_response(expected)
        ) as mock_get:
            result = client.get_nostr_prediction_events(market_id, agent_id)
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith(f"/v1/markets/{market_id}/predictions/{agent_id}/nostr")
            assert result["commit_event"]["kind"] == 30150

    def test_get_nostr_prediction_events_all_none(self):
        """Verify handling when no events exist yet."""
        client = RaijuClient(api_key="test")
        expected = {
            "agent_id": "a1",
            "market_id": "m1",
            "commit_event": None,
            "reveal_event": None,
            "attestation_event": None,
        }
        with patch.object(client.session, "get", return_value=_mock_response(expected)):
            result = client.get_nostr_prediction_events("m1", "a1")
            assert result["commit_event"] is None
            assert result["reveal_event"] is None
            assert result["attestation_event"] is None


class TestGetAgentAttestations:
    """Tests for the get_agent_attestations method."""

    def test_get_agent_attestations_correct_url(self):
        """Verify URL path includes agent_id."""
        client = RaijuClient(api_key="test")
        agent_id = "a-uuid-1"
        expected = {
            "agent_id": agent_id,
            "platform_pubkey": "f" * 64,
            "attestations": [{"kind": 30151, "content": "..."}],
        }
        with patch.object(
            client.session, "get", return_value=_mock_response(expected)
        ) as mock_get:
            result = client.get_agent_attestations(agent_id)
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith(f"/v1/agents/{agent_id}/nostr/attestations")
            assert len(result["attestations"]) == 1

    def test_get_agent_attestations_empty_list(self):
        """Verify handling when agent has no attestations."""
        client = RaijuClient(api_key="test")
        expected = {"agent_id": "a1", "platform_pubkey": "f" * 64, "attestations": []}
        with patch.object(client.session, "get", return_value=_mock_response(expected)):
            result = client.get_agent_attestations("a1")
            assert result["attestations"] == []


# ---------------------------------------------------------------------------
# Tests for other untested API methods (mock-based)
# ---------------------------------------------------------------------------


class TestHealthAndStatus:
    """Tests for health, status, and server_status methods."""

    def test_health_calls_correct_endpoint(self):
        """Verify health hits /v1/health."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response({"status": "ok"})
        ) as mock_get:
            result = client.health()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/health")
            assert result["status"] == "ok"

    def test_server_status_calls_correct_endpoint(self):
        """Verify server_status hits /v1/status."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response({"version": "0.5.0"})
        ) as mock_get:
            result = client.server_status()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/status")
            assert result["version"] == "0.5.0"

    def test_node_info_calls_status_endpoint(self):
        """Verify node_info hits /v1/status (same as server_status)."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session,
            "get",
            return_value=_mock_response(
                {"version": "0.5.0", "lightning_node_id": "03abc"}
            ),
        ) as mock_get:
            result = client.node_info()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/status")
            assert result["lightning_node_id"] == "03abc"

    def test_agent_status_includes_agent_id_in_path(self):
        """Verify status uses agent_id in the URL path."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response({"active": True})
        ) as mock_get:
            client.status("agent-123")
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/agents/agent-123/status")


class TestListMarkets:
    """Tests for list_markets with category parameter."""

    def test_list_markets_with_category(self):
        """Verify list_markets passes category as query param."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response([])
        ) as mock_get:
            client.list_markets(category="bitcoin")
            call_url = mock_get.call_args[0][0]
            assert "category=bitcoin" in call_url

    def test_list_markets_with_all_params(self):
        """Verify list_markets passes limit, offset, and category."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response([])
        ) as mock_get:
            client.list_markets(limit=10, offset=20, category="stocks")
            call_url = mock_get.call_args[0][0]
            assert "limit=10" in call_url
            assert "offset=20" in call_url
            assert "category=stocks" in call_url

    def test_list_markets_no_params(self):
        """Verify list_markets with no params hits bare endpoint."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response([])
        ) as mock_get:
            client.list_markets()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/markets")
            assert "?" not in call_url  # No query string


class TestListAgents:
    """Tests for list_agents method."""

    def test_list_agents_with_pagination(self):
        """Verify list_agents passes limit and offset."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response([])
        ) as mock_get:
            client.list_agents(limit=5, offset=10)
            call_url = mock_get.call_args[0][0]
            assert "limit=5" in call_url
            assert "offset=10" in call_url

    def test_list_agents_no_params(self):
        """Verify list_agents with no params hits bare endpoint."""
        client = RaijuClient(api_key="test")
        with patch.object(
            client.session, "get", return_value=_mock_response([])
        ) as mock_get:
            client.list_agents()
            call_url = mock_get.call_args[0][0]
            assert call_url.endswith("/v1/agents")
            assert "?" not in call_url  # No query string
