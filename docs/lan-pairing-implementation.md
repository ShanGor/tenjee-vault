# LAN exchange implementation

Status: development implementation, 2026-10-03. Commands, discovery, foreground lifecycle, authentication, mutual approval, replication, and conflict handling are connected. Starting exchange is disabled in release builds pending protocol acceptance. Compilation is not evidence of authentication correctness.

## Selected libraries

The implementation pins `opaque-ke` 4.0.1, `rustls` 0.23.45 with its ring provider, `rcgen` 0.14.10 with zeroization enabled, and `hmac` 0.12.1. The OPAQUE suite is Ristretto255 OPRF and 3DH with SHA-512 and Argon2 stretching. See the [OPAQUE implementation](https://github.com/facebook/opaque-ke/tree/v4.0.1) and [RFC 9807](https://www.rfc-editor.org/rfc/rfc9807).

The previously considered SPAKE2 Rust crate was removed after its secret-buffer handling did not meet the implementation's cleanup requirements. Selecting OPAQUE does not constitute a review of this application's composition or native-platform behavior.

## Handshake construction

1. Receiver creates an ephemeral self-signed certificate for `tenjee.local`; only TLS 1.3 is enabled. The connector verifies handshake signatures but provisionally accepts that certificate. This provisional TLS state is not authenticated peer trust.
2. Receiver sends a bounded greeting containing the exact application protocol identifier and random session UUID. The connector rejects a different protocol or invalid UUID.
3. Both compute the SHA-256 certificate fingerprint and a 32-byte TLS exporter using label `EXPORTER-tenjee-pairing-v1` and the session UUID as exporter context.
4. OPAQUE uses fixed connector/receiver identities. Its login context is a length-prefixed encoding of protocol identifier, session UUID, certificate fingerprint, exporter, and two empty message fields. The receiver registers the temporary password entirely in memory; network registration and persistent password files are not supported.
5. Exchange OPAQUE credential request, response, and finalization. Library deserialization checks the message structure. Authentication frames are capped at 2048 bytes before allocation; socket reads/writes have ten-second timeouts.
6. Both build a length-prefixed confirmation transcript of protocol, session UUID, certificate fingerprint, exporter, request, and response. Connector sends HMAC-SHA-256 over protocol, `connector`, and that transcript, keyed by the OPAQUE session key. Receiver verifies it and replies with the corresponding `receiver` confirmation. Both confirmations use constant-time MAC verification.
7. Return an authenticated TLS stream only after these steps succeed. Replica identities, schema versions, snapshot identity, and summaries are exchanged inside this stream before mutual scope approval; body requests begin only after both devices approve.

OPAQUE state/setup use the library's zeroization behavior; session/export keys are held in `Zeroizing` buffers. The session manager owns zeroizing eight-digit code strings, bounds attempts globally to five, expires codes after five minutes, consumes them on successful authentication, and shuts down sockets on stop, suspension, completion, or inactivity. Certificate/provider key cleanup and termination behavior still require protocol and native acceptance.

## Threat model and outstanding acceptance

- Passive observers see TLS ciphertext and public metadata, not the temporary code or workspace content. Short-code authentication must not permit passive offline guesses.
- An active relay that terminates separate TLS connections obtains different exporters/certificate bindings; OPAQUE context authentication must fail.
- Exact protocol matching and TLS-1.3-only configuration prevent fallback to weaker authentication. Fresh session IDs, TLS exporter binding, and fresh PAKE randomness must prevent handshake replay across sessions.
- Device labels and unverified mDNS results confer no trust. No unauthenticated workspace inventories/bodies or attachment bytes may be accepted.
- A hostile authenticated peer can still send malicious records; schema/size/path/hash/domain checks and transaction recovery belong to the replication boundary.
- Online guessing remains possible; the implementation enforces a five-failure global limit and five-minute expiry. Parallel-attempt and boundary behavior remain unverified.

Published OPAQUE vectors, current-library mutual authentication/wrong-code checks, substituted-channel/replay/downgrade checks, parallel attempt limits, expiry, stop/suspension cleanup, and mutual approval rejection remain pending. Earlier SPAKE2 test results do not satisfy OPAQUE acceptance. The release gate remains in place until the required protocol review and checks are complete. Development builds are for evaluation with disposable data.


## Session and replication boundary

- mDNS advertises only public device/session metadata on eligible local interfaces. Manual numeric IP/port connections must be in an eligible directly connected subnet. Device labels are descriptive, never proof of identity.
- The exact protocol identifier is `tenjee-lan-v1-opaque-ristretto255-sha512-tls13-entities2`. Schema versions are Meta 6, Tasks 8, Calendar 5, and Space 8. Both devices must use compatible builds. No automatic trust or reconnect is stored.
- Sessions stop after 30 minutes of inactivity. On Android, native onPause cancels the session even if JavaScript is suspended. Local source changes after the captured snapshot are reported for the next exchange.
- Typed allowlisted records exclude local paths/settings/permissions, unlock keys, delivered reminders, and local queues. Causal contexts, tombstones, complete protected-domain alternatives, and transaction receipts preserve offline changes and support repeated delivery.
- Protected group identity follows the root page. Titles/content/history/attachments transfer as ciphertext without unlocking. Legacy plaintext titles block the group until local unlock migrates them. Concurrent key transitions retain complete alternatives; resolution chooses a coherent version or duplicates a subtree with remapped owned references.
- A competing ordinary edit to a protected tree stays pending while locked. When a suitable local key is unlocked, bounded content is encrypted in memory before staging. Attachment conversion is limited to 64 MiB; larger competing plaintext attachments stay pending at their source.
- Attachment transfer uses 256 KiB chunks, SHA-256 verification, free-space checks, 512 MiB per-blob and 1 GiB staging/cache limits. Fresh pairing and approval are required to resume verified cached bytes. Entity data is capped at 64 MiB, inventory metadata at 64 MiB, and recovery metadata at 128 MiB to bound memory; larger workspaces are rejected explicitly.
- Ready manifests and per-store transaction receipts recover interrupted multi-database application. Only durably committed changes are acknowledged. Users can clear uncommitted pending transfer files while retaining committed data and original source edits.

See [phone validation](android-validation.md) for the deferred device scenarios. Current-library OPAQUE vectors, authentication/replay/channel-binding checks, malformed-peer checks, fault injection, and three-replica convergence are still required before production use.
