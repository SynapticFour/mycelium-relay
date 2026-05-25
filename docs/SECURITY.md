# Mycelium Security Model

## Transport security

All node-to-node connections use the Noise protocol (XX handshake pattern)
with Ed25519 identity keys. Traffic is encrypted in transit.

## Message authentication

Every message carries an Ed25519 signature from the originating node.
Relay nodes verify signatures before forwarding. Messages with invalid
or missing signatures are dropped (for encrypted direct messages) or
logged as warnings (for legacy unencrypted messages during transition).

Signature scheme v1 covers: message ID, sender peer ID, recipient peer ID,
payload, timestamp, TTL, priority, hop count, max hops.

## End-to-end encryption

Direct messages use X25519 ECDH key exchange with HKDF-SHA256 key derivation
and ChaCha20-Poly1305 AEAD encryption. Group messages use a pre-shared
symmetric key with ChaCha20-Poly1305.

**Limitation**: No forward secrecy. A stolen identity key allows decryption
of all past intercepted messages. Forward secrecy (Double Ratchet) is
planned for a future release.

## At-rest encryption

Identity keys and encryption keys are stored encrypted at rest using
ChaCha20-Poly1305 with a master key from:

1. Android Keystore-backed preferences (Android, preferred)
2. OS keyring (Desktop)
3. Random per-device file-based key (fallback)

The fallback is weaker than the Keystore/keyring options but stronger
than plaintext. Upgrade your OS to ensure keyring availability.

## Known limitations (beta)

- **No forward secrecy**: Static X25519 keys. Past messages can be
  decrypted if the key is compromised.
- **MeshCoin consensus**: The 3-witness model has no Sybil defense.
  MeshCoin is experimental and has no real monetary value.
- **Metadata**: Message sizes, timing, and routing patterns are visible
  to relay nodes even though content is encrypted.

## Reporting vulnerabilities

Please report security issues to: contact@synapticfour.com (Synaptic Four)
