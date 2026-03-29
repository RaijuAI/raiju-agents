"""Error handling tests for the Python SDK.

Verifies that the SDK returns proper errors for invalid operations.
"""

import uuid

import pytest
import requests

from raiju import RaijuClient


def test_deposit_nonexistent_market(agent_clients):
    """Depositing into a nonexistent market returns 404."""
    client, agent_id = agent_clients[0]
    fake_market = str(uuid.uuid4())
    with pytest.raises(requests.HTTPError) as exc_info:
        client.deposit(fake_market, agent_id, 5000)
    assert exc_info.value.response.status_code == 404


def test_commit_invalid_bps(agent_clients, open_market):
    """Committing with prediction_bps > 10000 raises ValueError."""
    client, agent_id = agent_clients[0]
    with pytest.raises(ValueError, match="prediction_bps must be 0-10000"):
        client.commit(open_market, agent_id, 10001)


def test_commit_negative_bps(agent_clients, open_market):
    """Committing with prediction_bps < 0 raises ValueError."""
    client, agent_id = agent_clients[0]
    with pytest.raises(ValueError, match="prediction_bps must be 0-10000"):
        client.commit(open_market, agent_id, -1)


def test_reveal_without_commit(raiju_url):
    """Revealing without a prior commit raises ValueError."""
    # Use a fresh client with no stored nonces
    uid = uuid.uuid4().hex[:8]
    client = RaijuClient(
        api_key="fake-key",
        base_url=raiju_url,
        nonce_dir=f"/tmp/raiju-test-nonces-empty-{uid}",
    )
    fake_market = str(uuid.uuid4())
    with pytest.raises(ValueError, match="No stored nonce"):
        client.reveal(fake_market, "fake-agent")


def test_status_nonexistent_agent(agent_clients):
    """Getting status for a nonexistent agent returns 404."""
    client, _ = agent_clients[0]
    fake_agent = str(uuid.uuid4())
    with pytest.raises(requests.HTTPError) as exc_info:
        client.status(fake_agent)
    assert exc_info.value.response.status_code == 404
