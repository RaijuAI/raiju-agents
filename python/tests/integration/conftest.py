"""Fixtures for Python SDK integration tests.

Requires a running Raiju server. Set environment variables:
  RAIJU_URL      - server URL (e.g. http://localhost:3001)
  DATABASE_URL   - PostgreSQL connection string (for admin bootstrap)

Optional (provided by sim framework):
  TEST_MARKET_ID   - pre-created market UUID
  TEST_OPERATOR_ID - pre-created operator UUID
  TEST_DEPOSIT_SATS - deposit amount matching market pool_entry_sats
"""

import os
import time
import uuid

import psycopg2
import pytest
import requests

# Adjust import path for the SDK
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))

from raiju import RaijuClient


@pytest.fixture(scope="session")
def raiju_url():
    """Server URL. Required."""
    url = os.environ.get("RAIJU_URL", "")
    if not url:
        pytest.skip("RAIJU_URL not set - start a server first")
    # Health check
    try:
        resp = requests.get(f"{url}/v1/health", timeout=5)
        resp.raise_for_status()
    except Exception as e:
        pytest.skip(f"Server not reachable at {url}: {e}")
    return url


@pytest.fixture(scope="session")
def db_url():
    """PostgreSQL URL for admin bootstrap."""
    url = os.environ.get("DATABASE_URL", "")
    if not url:
        pytest.skip("DATABASE_URL not set - needed for admin bootstrap")
    return url


@pytest.fixture(scope="session")
def admin_client(raiju_url, db_url):
    """RaijuClient with admin privileges.

    Creates an operator, sets is_admin=true via DB, returns client with
    the auto-created agent's API key.
    """
    uid = uuid.uuid4().hex[:8]
    # Create operator (auto-creates first agent)
    tmp_client = RaijuClient(api_key="", base_url=raiju_url)
    resp = tmp_client.register_operator(
        display_name=f"test-admin-{uid}",
        kind="autonomous",
    )
    operator_id = resp["id"]
    api_key = resp["agent"]["api_key"]

    # Set as admin via direct DB access
    conn = psycopg2.connect(db_url)
    try:
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE operators SET is_admin = true WHERE id = %s::uuid",
                (operator_id,),
            )
        conn.commit()
    finally:
        conn.close()

    client = RaijuClient(
        api_key=api_key,
        base_url=raiju_url,
        nonce_dir=f"/tmp/raiju-test-nonces-admin-{uid}",
    )
    return client, operator_id


@pytest.fixture(scope="session")
def agent_clients(raiju_url, admin_client):
    """Two non-admin agent clients for lifecycle testing.

    Returns list of (RaijuClient, agent_id) tuples.
    """
    _, admin_op_id = admin_client
    uid = uuid.uuid4().hex[:8]
    agents = []

    for i in range(2):
        # Create two independent agents for multi-agent tests
        tmp = RaijuClient(api_key="", base_url=raiju_url)
        resp = tmp.register_operator(
            display_name=f"test-agent-op-{uid}-{i}",
            kind="autonomous",
        )
        api_key = resp["agent"]["api_key"]
        agent_id = resp["agent"]["id"]

        client = RaijuClient(
            api_key=api_key,
            base_url=raiju_url,
            nonce_dir=f"/tmp/raiju-test-nonces-{uid}-{i}",
        )
        agents.append((client, agent_id))

    return agents


@pytest.fixture(scope="session")
def deposit_sats():
    """Deposit amount matching market pool_entry_sats."""
    return int(os.environ.get("TEST_DEPOSIT_SATS", "50000"))


@pytest.fixture(scope="session")
def open_market(raiju_url, admin_client, deposit_sats):
    """Create a market ready for deposits.

    If TEST_MARKET_ID is set (from sim framework), use that.
    Otherwise create a new market via admin endpoints.
    """
    pre_created = os.environ.get("TEST_MARKET_ID", "")
    if pre_created:
        return pre_created

    client, operator_id = admin_client
    uid = uuid.uuid4().hex[:8]

    from datetime import datetime, timezone, timedelta
    now = datetime.now(timezone.utc)

    # Create market with 30s commit, 30s reveal
    resp = client._post("/v1/markets", {
        "question": f"Python SDK integration test {uid}",
        "resolution_criteria": "integration test",
        "oracle_tier": "on_chain",
        "pool_entry_sats": deposit_sats,
        "commitment_open_at": now.isoformat(),
        "commitment_deadline": (now + timedelta(seconds=30)).isoformat(),
        "reveal_deadline": (now + timedelta(seconds=60)).isoformat(),
        "resolution_date": (now + timedelta(seconds=65)).isoformat(),
        "created_by": operator_id,
    })
    market_id = resp["id"]

    # Open market
    client._post(f"/v1/markets/{market_id}/open")

    # Init AMM
    b_param = 5000
    import math
    seed_sats = int(math.ceil(b_param * math.log(2) * 10000)) + 1
    client._post(f"/v1/markets/{market_id}/amm", {
        "b_parameter": b_param,
        "seed_sats": seed_sats,
        "lp_operator_id": operator_id,
    })

    return market_id


def poll_market_status(client, market_id, target_status, timeout=120):
    """Poll market until it reaches the target status."""
    start = time.time()
    while time.time() - start < timeout:
        market = client.get_market(market_id)
        status = market.get("status", "")
        if status == target_status:
            return market
        # Also accept later states (status progresses linearly)
        state_order = [
            "draft", "open", "commitment_closed", "revealing",
            "resolving", "resolved", "voided",
        ]
        if target_status in state_order and status in state_order:
            if state_order.index(status) > state_order.index(target_status):
                return market
        time.sleep(1)
    raise TimeoutError(
        f"Market {market_id[:8]} stuck at '{status}', "
        f"expected '{target_status}' within {timeout}s"
    )
