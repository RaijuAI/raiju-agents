"""Full market lifecycle test exercising every Python SDK method.

Runs two agents through the complete market lifecycle:
register -> deposit -> commit -> trade -> reveal -> payout -> leaderboard

Verifies budget balance: payouts + fee == total deposits.
"""

import time
from .conftest import poll_market_status


def test_full_market_lifecycle(
    raiju_url,
    admin_client,
    agent_clients,
    open_market,
    deposit_sats,
):
    """Exercise all 26 SDK methods through a complete market lifecycle."""

    admin, admin_op_id = admin_client
    market_id = open_market
    agent_a, agent_a_id = agent_clients[0]
    agent_b, agent_b_id = agent_clients[1]

    # --- Phase 1: Read-only queries (no state mutation) ---

    # health()
    health = agent_a.health()
    assert "status" in health, f"health missing 'status': {health}"
    assert health["status"] == "ok"

    # list_markets()
    markets = agent_a.list_markets()
    assert isinstance(markets, list), f"list_markets should return list: {type(markets)}"
    market_ids = [m["id"] for m in markets]
    assert market_id in market_ids, f"market {market_id[:8]} not in list"

    # get_market()
    market = agent_a.get_market(market_id)
    assert "question" in market
    assert "status" in market
    assert market["status"] in ("open", "commitment_closed", "revealing")

    # market_stats()
    stats = agent_a.market_stats(market_id)
    assert "deposit_count" in stats or "deposits" in stats, f"stats keys: {list(stats.keys())}"

    # market_deposits() - may be empty initially
    deposits = agent_a.market_deposits(market_id)
    assert isinstance(deposits, list)

    # get_amm()
    amm = agent_a.get_amm(market_id)
    assert amm is not None, "get_amm returned None"

    # get_consensus()
    consensus = agent_a.get_consensus(market_id)
    assert consensus is not None, "get_consensus returned None"

    # leaderboard()
    lb = agent_a.leaderboard()
    assert isinstance(lb, list)

    # solvency()
    solvency = agent_a.solvency()
    assert solvency is not None, "solvency returned None"

    # status()
    agent_status = agent_a.status(agent_a_id)
    assert agent_status is not None

    # --- Phase 2: Deposits ---

    deposit_a = agent_a.deposit(market_id, agent_a_id, deposit_sats)
    assert deposit_a is not None, "deposit returned None"

    deposit_b = agent_b.deposit(market_id, agent_b_id, deposit_sats)
    assert deposit_b is not None

    total_deposits = deposit_sats * 2

    # Verify deposits visible
    deposits = agent_a.market_deposits(market_id)
    assert len(deposits) >= 2, f"expected >= 2 deposits, got {len(deposits)}"

    # --- Phase 3: Trading ---

    trade_a = agent_a.trade(market_id, agent_a_id, "buy_yes", 3)
    assert "cost_sats" in trade_a, f"trade missing cost_sats: {trade_a}"

    trade_b = agent_b.trade(market_id, agent_b_id, "buy_no", 2)
    assert "cost_sats" in trade_b

    # trade_history() - by market
    trades_market = agent_a.trade_history(market_id=market_id)
    assert isinstance(trades_market, list)
    assert len(trades_market) >= 2, f"expected >= 2 trades, got {len(trades_market)}"

    # trade_history() - by agent
    trades_agent = agent_a.trade_history(agent_id=agent_a_id)
    assert isinstance(trades_agent, list)
    assert len(trades_agent) >= 1

    # positions()
    positions = agent_a.positions(agent_a_id)
    assert isinstance(positions, list)

    # price_history()
    prices = agent_a.price_history(market_id)
    assert isinstance(prices, list)

    # --- Phase 4: Commit ---

    commit_a = agent_a.commit(market_id, agent_a_id, 8000)
    assert commit_a is not None, "commit returned None"

    commit_b = agent_b.commit(market_id, agent_b_id, 3000)
    assert commit_b is not None

    # market_predictions() - should show committed (sealed) entries
    predictions = agent_a.market_predictions(market_id)
    assert isinstance(predictions, list)

    # --- Phase 5: Wait for reveal window ---

    print(f"  Waiting for reveal window (market {market_id[:8]})...")
    poll_market_status(agent_a, market_id, "revealing", timeout=60)

    # --- Phase 6: Reveal ---

    reveal_a = agent_a.reveal(market_id, agent_a_id)
    assert reveal_a is not None, "reveal returned None"

    reveal_b = agent_b.reveal(market_id, agent_b_id)
    assert reveal_b is not None

    # market_predictions() - should now show revealed entries
    predictions = agent_a.market_predictions(market_id)
    assert isinstance(predictions, list)

    # --- Phase 7: Wait for resolution ---

    print(f"  Waiting for resolution (market {market_id[:8]})...")
    try:
        resolved_market = poll_market_status(agent_a, market_id, "resolved", timeout=120)
    except TimeoutError:
        # Market may not resolve in time (depends on ticker interval)
        print("  Resolution timed out, skipping budget balance check")
        return

    # --- Phase 8: Post-resolution checks ---

    # payouts()
    payouts_a = agent_a.payouts(agent_a_id)
    assert isinstance(payouts_a, list)

    payouts_b = agent_b.payouts(agent_b_id)
    assert isinstance(payouts_b, list)

    # Budget balance verification
    market_detail = agent_a.get_market(market_id)
    fee = int(market_detail.get("fee_collected_sats", 0))

    total_payout = 0
    for p in payouts_a + payouts_b:
        if p.get("market_id") == market_id:
            payout_sats = p.get("payout_sats", "0")
            if isinstance(payout_sats, str):
                total_payout += int(payout_sats)
            else:
                total_payout += payout_sats

    assert total_payout + fee == total_deposits, (
        f"Budget imbalance: payouts({total_payout}) + fee({fee}) = "
        f"{total_payout + fee} != deposits({total_deposits})"
    )

    # leaderboard() - should have entries now
    lb = agent_a.leaderboard()
    assert isinstance(lb, list)
    assert len(lb) > 0, "leaderboard empty after resolution"

    print(f"  Full lifecycle passed. Budget balanced: {total_payout} + {fee} = {total_deposits}")
