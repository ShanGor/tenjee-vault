# Proposal

## Why

Tenjee Vault currently targets desktop and stores independent local workspaces. Users need a comfortable phone interface and a way to exchange changes between their own phone and laptop on the same local network while retaining offline use and avoiding a hosted service.

## What Changes

- Add Android and iOS targets with adaptive navigation, touch interactions, keyboard-aware editing, and platform-specific file and notification integration.
- Add an explicit “Find other devices and exchange” mode: discover a nearby device, enter its random temporary authentication code, approve the exchange on both devices, then synchronize in both directions.
- Encrypt and authenticate network traffic; keep protected page trees as ciphertext and never use pairing to unlock them.
- Synchronize logical records and attachments incrementally, including offline edits and deletions, with durable revision tracking, resumable transfer, and visible conflicts instead of silent overwrites.
- Document current desktop support separately from the planned mobile and LAN capabilities. Internet/cloud sync, accounts, collaboration, and unattended background sync remain outside this change.

## Capabilities

### New Capabilities

- `mobile-experience`: Android/iOS delivery and adaptive layouts for phones and tablets.
- `local-network-sync`: Opt-in discovery, code pairing, authenticated exchange, merge/conflict behavior, and session lifecycle.

### Modified Capabilities

- `data-storage`: Durable causal revisions, deletion tombstones, replication checkpoints, and restore-safe replica identity.
- `crypto-core`: Ciphertext-preserving replication of protected page trees without transferring unlocked session secrets.

## Impact

Frontend: app shell, notes navigation/editor, tasks/detail/creation, calendar, settings, localization, and sync/conflict views. Rust: platform-gated desktop plugins, mobile permissions/lifecycle, new discovery/transport/replication modules, and migrations across meta/tasks/calendar/space databases. All mutation paths, attachments, backup/restore, and existing task-to-note queues need replication integration. Discovery and established PAKE/secure transport dependencies require compatibility and security review before implementation. The existing page-tree and medication-task changes are already implemented and must remain supported; this proposal does not rewrite those changes or claim that mobile/sync has shipped.

Protected titles will use explicit ciphertext storage. Existing plaintext titles in protected trees migrate transactionally after successful local unlock; those trees remain pending for exchange until migrated.
