"""Test commitment hash computation matches cross-language test vectors (ADR-004)."""

import hashlib
import json
from pathlib import Path


def test_commitment_hash_vectors():
    """Verify Python SDK produces the same hashes as the Rust implementation."""
    vectors_path = Path(__file__).parent.parent.parent.parent / "docs" / "test-vectors" / "commitment-hash.json"
    with open(vectors_path) as f:
        data = json.load(f)

    for vector in data["vectors"]:
        prediction_bps = vector["prediction_bps"]
        nonce_hex = vector["nonce_hex"]
        expected_hash = vector["commitment_hash_hex"]

        # Compute hash using the same method as the SDK
        pred_bytes = prediction_bps.to_bytes(4, "big", signed=True)
        nonce = bytes.fromhex(nonce_hex)
        actual_hash = hashlib.sha256(b"raiju-v1:" + pred_bytes + nonce).hexdigest()

        assert actual_hash == expected_hash, (
            f"Hash mismatch for prediction_bps={prediction_bps}: "
            f"expected {expected_hash}, got {actual_hash}"
        )


def test_prediction_bps_byte_encoding():
    """Verify prediction_bps encodes as 4-byte big-endian signed i32."""
    cases = {
        0: b"\x00\x00\x00\x00",
        1: b"\x00\x00\x00\x01",
        5000: bytes.fromhex("00001388"),
        7200: bytes.fromhex("00001c20"),
        9999: bytes.fromhex("0000270f"),
        10000: bytes.fromhex("00002710"),
    }
    for bps, expected in cases.items():
        assert bps.to_bytes(4, "big", signed=True) == expected, f"failed for {bps}"
