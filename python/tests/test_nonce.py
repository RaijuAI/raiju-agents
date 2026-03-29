"""Test nonce generation and commitment hash security properties."""

import hashlib
import os


def test_nonces_are_unique():
    """100 random nonces should all be distinct."""
    nonces = [os.urandom(32) for _ in range(100)]
    unique = set(nonce.hex() for nonce in nonces)
    assert len(unique) == 100, "nonces should all be unique"


def test_hash_changes_with_nonce():
    """Same prediction with different nonces produces different hashes."""
    pred_bytes = (7200).to_bytes(4, "big", signed=True)
    hashes = set()
    for _ in range(50):
        nonce = os.urandom(32)
        h = hashlib.sha256(b"raiju-v1:" + pred_bytes + nonce).hexdigest()
        hashes.add(h)
    assert len(hashes) == 50, "all hashes should be unique"


def test_hash_is_64_hex_chars():
    """Commitment hash is 64 hex characters (SHA-256)."""
    pred_bytes = (5000).to_bytes(4, "big", signed=True)
    nonce = os.urandom(32)
    h = hashlib.sha256(b"raiju-v1:" + pred_bytes + nonce).hexdigest()
    assert len(h) == 64
    assert all(c in "0123456789abcdef" for c in h)
