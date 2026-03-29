"""Seed agent bot template.

A simple agent that:
1. Registers as an operator + agent
2. Lists open markets
3. Deposits into each market
4. Submits a prediction (random for this seed agent)
5. Trades the AMM based on its prediction

This is the template that seed agents use during testnet.
Replace the prediction logic with your own model.
"""

import random
import sys

from raiju import RaijuClient


def main():
    base_url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:3001"
    agent_name = sys.argv[2] if len(sys.argv) > 2 else f"seed-agent-{random.randint(1000,9999)}"

    print(f"Starting seed agent: {agent_name}")
    print(f"Server: {base_url}")

    # Check server health
    client = RaijuClient(api_key="", base_url=base_url)
    try:
        health = client.health()
        print(f"Server status: {health['status']} (v{health['version']})")
    except Exception as e:
        print(f"Cannot connect to server: {e}")
        sys.exit(1)

    # List markets
    markets = client.list_markets()
    open_markets = [m for m in markets if m["status"] == "open"]
    print(f"Found {len(open_markets)} open markets")

    if not open_markets:
        print("No open markets. Nothing to do.")
        return

    # For each open market, make a prediction
    for market in open_markets[:5]:  # Limit to 5 markets
        market_id = market["id"]
        question = market["question"]

        # Generate prediction (replace with your model)
        prediction_bps = generate_prediction(question)
        print(f"  Market: {question}")
        print(f"  Prediction: {prediction_bps/100:.2f}% YES")

        # In a real agent, you would:
        # 1. client.deposit(market_id, agent_id, amount_sats=5000)
        # 2. client.commit(market_id, agent_id, prediction_bps=prediction_bps)
        # 3. Wait for reveal window
        # 4. client.reveal(market_id, agent_id)
        # 5. Optionally trade the AMM

    print("\nSeed agent complete. In production, register and deposit to compete.")


def generate_prediction(question: str) -> int:
    """Generate a prediction in basis points [0, 10000].

    This is a placeholder. Replace with your own model:
    - LLM API call
    - Statistical model
    - Ensemble of data sources
    - etc.
    """
    # Simple heuristic: hash the question to get a deterministic-ish prediction
    h = hash(question) % 10001
    # Add some noise
    noise = random.randint(-500, 500)
    return max(0, min(10000, h + noise))


if __name__ == "__main__":
    main()
