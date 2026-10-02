# Design

## Context

See proposal.md for motivation. The app uses React 19/TipTap in Tauri 2, Rust commands, rusqlite, separate meta/tasks/calendar databases, one database per notes space, and content-addressed attachment files. The existing page tree retains private section records as crypto domains. These internal records and sentinel IDs are storage details, not additional navigation layers. The current mobile CSS wraps desktop navigation and allows notes/tasks horizontal overflow; Month has a 700 px minimum width. The Tauri entry point already has a mobile attribute, but desktop plugins, tray/window behavior, and setup run unconditionally. No LAN discovery, replication metadata, or mobile target project exists.

Existing task-to-note status propagation is a local queue/trigger, not peer replication. Main OpenSpec notes requirements still describe the old hierarchy; the implemented simplify-notes-to-page-tree change is the current UI/data baseline. Preserve its IDs/domains and medication-course behavior. README's desktop-only statements describe the current release; the old product spec's purely offline/no-multidevice scope is superseded only for the planned release.

## Goals / Non-Goals

Goals: reuse local-first domain logic across five platforms; make small screens usable; perform intentional two-device exchanges that converge safely after offline work; preserve protected ciphertext and recover from interruptions.

Non-goals: hosted accounts, cloud/WebDAV/Internet transport, continuous background sync, multiplayer editing, automatic trust retention, or replacing a workspace with a remote backup. First release exchanges the complete workspace; selective space/domain sharing is deferred to avoid ambiguous cross-module references and deletions.

## Decisions

### Adaptive shell and mobile platform adapters

Use the existing React/Rust application with Tauri Android/iOS targets. A PWA would need a separate browser storage/native command replacement; a separate UI stack would duplicate editor and domain behavior. Phone layout is 320–600 CSS px, tablet 601–900, and wide layout above 900, with container-aware adjustments where needed. Device platform gates native capabilities; viewport size controls layout, so a narrow desktop also gets the compact UI.

Bottom navigation provides Notes/Tasks/Calendar/More; search and quick capture remain reachable. More contains Tags, Settings, and exchange. Notes use a space/page-tree drawer and read-first full-width document. Tasks use list selection and full-screen details/forms; mobile Kanban has one column and status navigation. Calendar defaults to Agenda, uses a compact month picker plus selected-day list, and offers Day and readable Week summaries. Keep separate compact/wide calendar preferences. Overflow is limited to document tables or code regions, never the whole module. Explicit move, reorder, date, and status controls cover operations currently dependent on dragging/hover.

Use dynamic viewport sizing, safe-area insets, keyboard-aware editor controls, accessible labels/focus, 44 px touch targets, and 16 px inputs. Gate tray, global shortcuts, single-instance/window-state behavior, and desktop close interception. Use mobile sandbox storage, document pickers/share integration, permission handling, and OS-scheduled reminders. Native clipboard/file/notification APIs must be checked against actual Android/iOS support rather than assumed identical to desktop. iOS build/release work requires a macOS/Xcode runner.

### Temporary discovery and pairing

Both devices explicitly enter exchange mode. The receiver opens a local-interface-only ephemeral TCP endpoint and advertises a versioned mDNS/DNS-SD service with a temporary session ID, user-visible device label, platform, and port. Do not publish content, codes, entity counts, persistent replica identifiers, or crypto-domain names. Provide a manual local IP/port fallback. Validate addresses and interfaces, including local IPv6/link-local scopes; reject public, routed Internet, and redirect endpoints. Do not enable UPnP/NAT mappings. The same router does not guarantee connectivity: guest isolation, firewalls, VPN routing, multicast blocking, and denied LAN permissions get actionable UI.

The receiver generates eight digits uniformly with the OS CSPRNG; leading zeros are valid. Expiry is five minutes. A global per-code counter invalidates after five failed attempts, regardless of source IP; serialize authentication to prevent race/parallel guessing. Successful mutual authentication consumes the code and stops accepting other peers. No persistent pairing secret is stored; every new exchange pairs again. Last-sync summaries and peer checkpoint identities are data history, not authorization.

### Authenticated encrypted transport

Use an established password-authenticated key exchange implementation with explicit mutual confirmation; SPAKE2 as described in RFC 9382 is a candidate, subject to implementation review. Bind the handshake to protocol/application identity, initiator/receiver roles, temporary session ID, both replica identities after authentication, and the secure transport channel. Use TLS 1.3 with ephemeral certificates and PAKE-authenticated certificate/channel binding; an initially self-signed connection is provisional and grants no data authority until the PAKE and channel binding succeed. Never implement an ad hoc “hash the code and send it” or treat an unverified TLS certificate as peer authentication. Pin and review the selected Rust PAKE/TLS implementations and channel-binding construction before release; library selection does not relax the specified security behavior.

Only bounded pairing messages are accepted before authentication. After authentication, exchange compatible schema versions and count/hash manifests sufficient for the preview; no page bodies or attachment bytes precede mutual approval. Both approve full-workspace exchange. Authentication codes and session keys are volatile and excluded from logs, crash diagnostics, backups, and disk. Finish, stop, inactivity, or suspension closes sockets/advertisements and zeroizes secrets. A fresh authenticated session is necessary to resume durable progress. OS LAN permission prompts and narrowly scoped desktop firewall guidance occur only when entering mode.

### Logical replication and causal history

Replicate versioned entity envelopes, not SQLite files or a .tvault restore. An envelope identifies logical entity type/ID and owning space, origin replica, monotonic origin counter, causal predecessors/context, payload schema, deletion state, attachment references, and content hashes. Revision metadata is trusted only after authentication and structural validation. Timestamps remain display information. Use durable per-entity causal context (for example dotted version vectors) to distinguish a successor from concurrent branches, and per-origin counters for duplicate detection and delta cursors.

Create revision metadata/change journals in each owning database. Local mutation and its revision commit in the same transaction. Cover every writer, including imports, hierarchy/protection changes, batch/archive/delete/restore, recurrence and medication courses, templates, tag links, and attachments. Materialized FTS, session indexes, device reminder-fire history, task-to-note queues, absolute paths, local preferences, and backup settings are reconstructed or retained locally, not transmitted. Wrapped keys and crypto-domain IDs travel with protected content. Internal sentinel records such as __plain_pages__ are canonical storage scaffolding scoped to their space; do not treat them as globally unique user entities. Independently initialized default spaces/lists with different IDs remain distinct rather than merged by name.

First exchange inventories full logical state. Subsequent exchanges compare causal heads and acknowledged revision ranges and request only missing revisions/blobs. Preserve origins when applying incoming records rather than emitting the same edit as a new local mutation. Local derived changes such as linked-checkbox updates are idempotent semantic operations associated with their triggering revision; missing/locked page work remains local queued work and cannot cause echo loops. Three-device A↔B↔C exchange must retain provenance and forward received revisions. Changes after the approved snapshot cutoff are queued for the next exchange, and the result explicitly shows pending new local changes.

### Conflicts and protection boundaries

Causally later revisions supersede their ancestors; concurrent revisions retain all alternatives. First version uses whole-entity conflicts instead of rich-text merging or timestamp-based last-write-wins. Edits to different entities merge automatically. An unchanged old record loses to its causally newer deletion; concurrent edit/delete produces a recoverable conflict. Preserve competing ordering/moves as variants and validate tree acyclicity and inherited protection before materializing. Conflict IDs and variant ordering are deterministic so repeat exchange does not create conflict copies.

Users can select a variant or keep both. Resolution creates a new revision covering every competing head. Keeping both allocates a new ID for the duplicate and updates only references inside that duplicated subtree; unrelated inbound links continue to refer to the original. An unresolved conflict may show the last common committed version, or a deterministic variant if there is no common version, but must show its conflict state and keep the others durable.

Treat concurrent protection/domain membership, wrapped-key changes, rekeying, and ciphertext edits as coherent protected-domain snapshots with complete referenced blobs. Do not mix a new wrapping key with incompatible ciphertext or let a move leak decrypted content into an ordinary branch. An ordinary-to-protected conversion racing with a plaintext edit requires local unlock to encrypt the ordinary variant before committing it as a protected conflict. Without an available key, pause that dependency group before transferring/persisting the competing body, retain the original revision on its source, and show it as pending; other independent groups can finish. This avoids creating a new plaintext conflict file for a protected tree. Source ciphertext, including encrypted titles, is used even if the source is unlocked. Destination remains locked. Invalidate affected source/destination unlock sessions before changing key/domain metadata. Conflicts requiring more than one password ask for the relevant passwords locally; pairing codes never substitute for protection passwords.

### Staging, checkpoints, and recovery

Persist exchange manifests and validated chunks in a bounded staging area, with no decrypted protected data. Default maximum protocol frame/record size is 16 MiB; larger logical content uses chunked blobs. Attachment chunks are at most 1 MiB. Validate counts, total sizes, free space, types, hashes, schema versions, IDs, and references before application; limit outstanding streams and staged bytes. Verify each full blob before promoting it to content-addressed storage or exposing a referencing record. Reject traversal paths, executable commands, unexpected file destinations, and oversized declarations.

Multi-database application uses a durable coordinator manifest and idempotent per-database batches, since the current architecture has no cross-database transaction. Stage dependency groups, then commit each owning database with revisions/checkpoints. Surface exchange as partial until every required group completes; recover outstanding groups at next startup. Never acknowledge uncommitted revisions. Committed valid changes remain if cancelled; cancellation is not a rollback of already applied records. Do not delete staging or attachments still needed by conflicts/recovery. Tombstones and causal deletion knowledge have no age-based expiry in the initial release, preventing an arbitrarily old offline replica from resurrecting data.

Local restore remains a separate destructive operation. Sync metadata is included in compatible backups, but restore/clone creates a fresh local replica identity and clears transport authorization, resets peer cursors, and reconciles inventory before resuming. Stable entity IDs and prior causal history remain. Keep sequence counters monotonic per replica; do not reuse the backed-up installation identity on a second device. Older backups without replication metadata initialize a baseline.

## Risks / Trade-offs

- Short code and hostile LAN → PAKE, transport binding, global attempt bounds, mutual approval, and protocol review; device labels are descriptive, not proof of identity.
- Missing mutation paths or local queue feedback → central journal integration, transaction tests, and three-device/linked-checkbox convergence fixtures.
- Concurrent protected transitions → preserve coherent protected snapshots and defer resolutions requiring unavailable keys; no silent overwrite or decrypt-for-sync fallback.
- Unbounded offline duration → retain tombstones/causal context; storage compaction needs a separate explicit replica-retirement design.
- Mobile suspension and notification differences → foreground exchange, fresh pairing to resume, OS reminder scheduling, and real-device checks.
- First large exchange consumes time/storage → chunking, disk preflight, progress, cancellation, and resumable verified staging.

## Migration Plan

1. Keep v1.0 desktop behavior documented as released; introduce the new requirements as a future change.
2. Implement adaptive UI and platform adapters, build Android/iOS on supported runners, and retain desktop smoke checks.
3. Back up before sync migrations. Add per-database causal/journal tables and baseline existing objects transactionally without changing entity IDs, protected ciphertext, or wrapped keys.
4. Add the transport/discovery and replication engine behind the explicit exchange entry point. Validate pairing, migrations, protected domains, recovery, and convergence before enabling production exchange.
5. Release only after real phone/laptop exchange checks pass. Older binaries must not open a newer unsupported sync schema; rollback uses a verified pre-migration backup. Sync is not a recovery/restore mechanism.

## References

- [Tauri platform overview](https://v2.tauri.app/start/) and [plugin support table](https://v2.tauri.app/plugin/).
- [Tauri mobile prerequisites](https://v2.tauri.app/start/prerequisites/).
- [RFC 9382: SPAKE2](https://www.rfc-editor.org/rfc/rfc9382), describing password-authenticated key exchange and mutual key confirmation.
