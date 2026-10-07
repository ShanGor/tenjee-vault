# Files and folders in Device Exchange

Files & folders is available in development builds on Linux, Windows, and Android. Release builds retain Vault data exchange; file mode remains gated until the physical-device and network acceptance work in the [validation report](../openspec/changes/device-file-exchange/validation.md) is complete. macOS and iOS file mode are unavailable.

## Send a batch

1. Open Device Exchange and select **Files & folders**, then **Send files**.
2. Use **Add files** for one or several files and **Add folder** for a directory. You can mix selections and remove them before preparing.
3. Select **Prepare and preview**. Review the paged relative paths, byte totals, and exclusions. Acknowledge exclusions before entering exchange.
4. On the recipient, select **Receive files** and choose a destination folder before entering exchange.
5. Enter exchange on both devices. Select a reachable network and pair through discovery or a manual hostname/IP and port. Enter the listener's temporary code on the connecting device.
6. Check the authenticated peer, direction, manifest, and destination naming plan, then approve on both screens. Keep both applications open until the recipient confirms completion.

The listener can send or receive. A batch travels in one direction; use a new batch to send back. File mode copies selected content independently of the vault database. It does not import files into notes or synchronize later filesystem edits.

## Selection and saving rules

Folder structure, hidden files, empty folders, and zero-byte files are preserved. Overlapping selections are deduplicated. Duplicate top-level names receive a deterministic suffix. Symlinks, Windows reparse points, special files, and unreadable entries are excluded without following them. Unsupported portable names, non-UTF-8 names, and case/Unicode collisions stop preparation with guidance to rename or remove a selection. Permissions, owners, extended attributes, and timestamps are not replicated.

The implementation bounds a batch to 100,000 entries, a 64 MiB estimated manifest/recovery metadata budget, and 1,000 selected roots. Files stream through 1 MiB chunks and 8–32 MiB flow-control windows. Native handles carry file data; the WebView receives metadata and status. Files larger than 4 GiB use exact 64-bit totals.

The recipient creates a new `Tenjee Received …` batch folder. Existing files are never overwritten or merged into an unexpected directory. Desktop partial files remain in that batch's private `.tenjee-partials` directory until verified and published. A source changed during preparation, hashing, or transfer stops the batch. Received files are never launched automatically; **Open destination** opens the folder only when requested.

## Android document providers

Finish all source and destination pickers before pairing. Android uses the system document/tree picker and its returned grants, without broad storage permission. Keep the app in the foreground: backgrounding or activity suspension stops authorization and closes the transfer. Keep-screen-on is scoped to the active exchange.

Seekable, stable documents stream from native descriptors. Some providers expose unknown lengths, changing descriptors, or non-seekable streams. The sender must explicitly allow per-file private staging for those sources; preparing then needs space for the staged copies. Staged sources persist for resume. Unsupported documents appear as exclusions.

Receiving uses app-private staging for the current file, then copies it to the chosen provider and reads it back to verify its length and hash. **Saving** can continue after network progress reaches 100%. Provider capacity and durable/atomic publication guarantees may be unknown. Some providers show the final document name while its copy is incomplete. Interrupted owned copies remain identifiable for retry or discard; completion is recorded only after successful verification. Grant revocation or provider refusal requires user action. Actual provider and phone lifecycle acceptance remains pending.

## Stop, resume, and discard

**Pause / Stop** ends the current authorization. Durable progress remains; bytes beyond the last flushed and journaled checkpoint are rechecked or discarded. Saving, completed counts, durable bytes, and in-flight bytes are separate from final confirmation. A missing batch acknowledgement is **Unconfirmed**, even if file receipts arrived.

To resume, the sender chooses the original batch from history. The recipient chooses the original destination again. Pair with a fresh code and approve on both devices. Retained chunks and completed outputs are verified before being skipped; unchanged prefixes do not travel again. Changed sources require a new offer, and missing/edited completed outputs produce an error rather than an overwrite. There is no automatic reconnect or permanent peer trust.

Transfer records live in app-private `file-exchange/transfers.sqlite`, outside vault replication and backups. Android's private transfer files/provider ledger use `file-exchange-native`, separately from the ten-minute import/share cache. Pending records and staging have no automatic age-based deletion and remain beyond seven days until explicitly discarded. History shows the latest 100 batches. This can consume storage; use **Discard incomplete files** when needed. Discard removes only batch-owned partials, recovery records, and eligible grants. Source files, completed output, and unrelated destination files remain.

## LAN, Tailscale, and other VPNs

Both devices must have a reachable TCP route. Multicast discovery usually stays on the local LAN and may be unavailable on a routed VPN. Enter the peer's VPN IP or resolvable hostname manually, including Tailscale MagicDNS where configured. Use `[IPv6-address]:port` for an explicit IPv6 address and port. Select the appropriate interface/address; a fixed listening port can simplify firewall and access rules. File mode never silently changes route or falls back to Vault data mode.

For connection failures, check the listener's address/port, OS firewall, VPN access rules, interface availability, and DNS resolution. An occupied fixed port must be changed or freed. Pairing codes expire after five minutes and permit five failed attempts; an authenticated code cannot be reused. Approval waiting and payload progress have separate timeouts. A payload stalled for two minutes ends the session; idle approval eventually expires as well. Start a fresh session after an interruption.

Tailscale is an optional network transport. Tenjee does not require its CLI or create a relay service. Throughput depends on the actual route, disk, CPU, and provider. Local 4 GiB, 8 GiB, and 10,000-file measurements, their reproducible commands, and the untested LAN/VPN profiles are in the [validation report](../openspec/changes/device-file-exchange/validation.md).

## Development and rollback

Use `npm run tauri -- dev` for desktop development and `npm run android:apk -- --ci` for an ARM64 debug APK. Android's tracked native sources are described in [Android development](android-development.md). The separate [OpenSpec change](../openspec/changes/device-file-exchange/proposal.md) contains the design and remaining acceptance tasks.

Rollback keeps Files & folders unavailable through `file_exchange_available_cmd` and preserves Vault data exchange's legacy authentication/protocol identifier. Retain transfer journals and owned files if rolling back to a build that cannot resume them; do not run generic vault/import cache cleanup over transfer storage. No vault-schema migration is required for file mode.
