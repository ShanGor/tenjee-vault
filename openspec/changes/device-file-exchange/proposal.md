# Proposal

## Why

Device Exchange currently transfers Tenjee Vault workspace data, but users also need to send ordinary files, multiple files, and complete folders between their devices. Extending the existing connection and pairing experience will provide fast, recoverable transfers over reachable LAN and private VPN networks, including Tailscale, without a hosted file service.

## What Changes

- Add two explicit Device Exchange modes: **Vault data** and **Files & folders**. Preserve existing bidirectional vault exchange; file transfer is an intentional one-way copy to one receiving device per session.
- Let senders build a batch using **Add files** (single or multiple) and **Add folder**, combine selections, review nested files/empty folders and total size, remove items, and approve the final offer. Recipients review the offer and choose a local destination before accepting.
- Preserve folder structure and valid filenames; receive into a new batch directory by default. Explain unsupported entries and naming conflicts before approval; never silently overwrite existing files or automatically open received content.
- Reuse explicit network selection, LAN discovery, manual hostname/IP and port entry, and temporary authenticated pairing. Support Tailscale MagicDNS/IP and routed private VPN peers without relying on multicast discovery across VPNs.
- Add a bounded binary streaming protocol, integrity verification, durable checkpoints, fresh-pairing resume, progress/speed/ETA, cancellation, and accurate partial-completion reports. Remove vault attachment size/cache ceilings from the new file mode while keeping independent resource limits.
- Cover desktop platforms and the existing Android development target with native file/tree selection and destination access. Retain foreground-only authorization; iOS support remains a later mobile integration.

## Capabilities

### New Capabilities

- `device-file-exchange`: Mode separation, selected file/folder batches, recipient consent, LAN/private VPN connectivity, bounded encrypted streaming, safe destination publication, interruption recovery, platform access, and measurable transfer acceptance.

### Modified Capabilities

None. `local-network-sync` and `mobile-experience` currently exist in the unarchived `mobile-and-lan-sync` change, rather than the main spec inventory. This additive capability depends on their implemented pairing/network boundary and preserves their vault behavior. Do not duplicate or overwrite those active change artifacts; reconcile shared session integration when applying this change.

## Impact

- Frontend: `src/shared/ExchangePage.tsx`, shared transfer state/components, translated labels, native selection integration, and receive/resume UI.
- Rust: `src-tauri/src/sync/{session,auth,network,files}.rs` integration plus a separate file-transfer engine, bounded codec, selection handles, and durable local transfer journal. `sync/engine.rs` stays the vault replication boundary.
- Native files: `src-tauri/src/mobile/{files,mod}.rs` and the existing Android `NativeServicesPlugin.kt`; add document-tree and descriptor/provider streaming support instead of whole-file base64 IPC or mandatory batch copies.
- Local storage: transfer-only metadata and partial files, excluded from vault replication and backups. Received files are ordinary destination files, not vault attachments or encrypted vault records.
- Dependencies: reuse TLS 1.3/OPAQUE, hashing, networking, SQLite, and desktop dialogs; a narrowly scoped native filesystem dependency may be needed for safe directory-relative opens and publication. No embedded Tailscale SDK, account, cloud relay, QUIC requirement, or archive-first pipeline.
- Compatibility: retain the existing vault wire protocol. File mode has a distinct authenticated protocol identifier; unsupported peers receive a mode/version explanation without falling back to vault exchange.
- Validation: fault injection and native filesystem tests; actual desktop/Android transfers over LAN, Tailscale, and routed VPN; comparative throughput and memory benchmarks. Existing unresolved authentication/native acceptance in `mobile-and-lan-sync` remains outstanding and must be covered for the reused boundary.
