# Tasks

## 1. File-mode boundaries and compatibility

- [x] 1.1 Introduce the separate file-exchange module, typed Send/Receive intent, and backend-owned selection/destination handles without acquiring vault snapshot locks; verify file preparation leaves vault records and existing vault commands unchanged.
- [x] 1.2 Parameterize the allowlisted authentication protocol binding while preserving the exact legacy vault identifier; verify file/vault mode mismatch and unsupported peers fail before content, and legacy vault pairing still succeeds.
- [x] 1.3 Extend public discovery hints and authenticated file hello with protocol, intent, platform, and bounded capabilities; verify role conflicts and spoofed discovery hints cannot select another authorized mode.
- [x] 1.4 Add a versioned transfer-only journal outside replicated/backed-up vault data and Android's legacy import cache; verify independent journal initialization/recovery and no transfer metadata appears in a vault snapshot or backup.

## 2. Native selection and immutable offers

- [x] 2.1 Implement desktop multiple-file/folder selection and incremental no-follow scanning with hidden files, empty directories, overlap deduplication, and bounded handles; verify a mixed tree yields exactly the previewed supported entries and explicit exclusions.
- [x] 2.2 Implement bounded paged manifest persistence, canonical digesting, checked 64-bit totals, and predicted recovery-metadata cost; verify stable digests, overflow/limit rejection, and byte-count display above JavaScript's safe integer boundary.
- [x] 2.3 Create deterministic batch-root/name mapping with platform naming/collision checks and sender acknowledgement of exclusions; verify duplicate basenames, Unicode/case normalization collisions, reserved names, and file/directory conflicts appear before approval.
- [x] 2.4 Implement authenticated offer exchange and mutual approval bound to session, direction, batch, manifest, and naming plan; verify approval changes/declines prevent all payload transfer and local paths/grants never appear on the wire.

## 3. Binary streaming and resource control

- [x] 3.1 Implement the bounded binary file codec and state validation separately from legacy JSON vault frames; verify truncated/oversized/unknown frames, duplicate IDs, invalid offsets, and integer overflow are rejected before allocation or writes.
- [x] 3.2 Stream raw source bytes with bounded read-ahead, source identity/version checks, chunk hashes, and streaming whole-file hashes; verify zero-byte, multi-chunk, 8 GiB, and edited-source cases without whole-file buffering or base64 IPC.
- [x] 3.3 Implement ordered data windows, grouped checkpoint/receipt replies, adaptive 8–32 MiB credit, and bounded pending small-file completions; verify no per-chunk/per-file roundtrip, bounded queues, and no sender/receiver deadlock under backpressure.
- [x] 3.4 Add phase-specific socket polling/watchdogs, approval waiting states, immediate socket shutdown on Stop, and preflight/ongoing destination-space checks; verify slow-progress transfer, stalled-peer timeout, delayed approval, cancellation, and disk exhaustion produce accurate results.

## 4. Safe output and durable publication

- [ ] 4.1 Implement pinned directory-relative destination access and exclusive batch-root creation for Unix and Windows, selecting only the narrow native dependency needed; verify symlink/reparse races, traversal, UNC/drive paths, alternate streams, and existing targets cannot escape or be overwritten.
- [x] 4.2 Receive to owned partials with incremental integrity validation and grouped file-flush-before-journal checkpoints; verify durable offsets never precede persisted bytes and a process kill discards only uncheckpointed tails.
- [ ] 4.3 Implement full hash verification, ready-to-publish records, atomic no-replace publication, native directory durability, and completed receipts; verify failures/crashes at each publication boundary preserve prior files and recover the correct owned output.
- [x] 4.4 Add empty-directory finalization and final recipient acknowledgement with Complete/Partial/Unconfirmed results; verify missing acknowledgement never becomes sender success, accepted-entry failures remain explicit, and output is never automatically launched.

## 5. Recovery, resume, and owned cleanup

- [x] 5.1 Persist random sender batch capability and approved source/destination identities, and validate retained bytes on recovery; verify guessed batch IDs, labels, wrong capabilities, altered partials, and changed destinations cannot reuse another batch's progress.
- [x] 5.2 Implement fresh-pairing/mutual-approval resume with current-source prefix verification and hasher reconstruction; verify unchanged prefixes skip network retransmission, corrupted ranges roll back, and changed source requires an explicit restart/new offer.
- [x] 5.3 Reconcile publication/receipt gaps and revalidate completed output before skipping it; verify a lost final receipt produces no duplicate and an edited/missing destination file is not silently overwritten.
- [ ] 5.4 Implement ownership-checked discard and pending retention of at least seven days, with explicit low-storage discard and scoped native grant release; verify cleanup preserves sources/completed output and startup cache cleanup cannot delete resume artifacts.

## 6. Android native document and tree adapters

- [x] 6.1 Keep Android services changes in a reproducible tracked native source/build scaffold and add multiple-document/source-tree/destination-tree handles with returned URI grants; verify a clean generated Android project includes the adapter and pickers require no broad storage permission.
- [ ] 6.2 Enumerate provider trees with bounded native access, exclusions, and descriptor ownership; bridge seekable reads or bounded native streams without WebView payloads; verify nested/hidden files, empty folders, revoked grants, descriptor closure, and large documents.
- [ ] 6.3 Add disclosed per-file staging for non-seekable/unknown-size sources and app-private active-file staging for receiving; verify space accounting, stable staged-source resume, and unsupported-provider guidance without legacy 1 GiB import limits.
- [ ] 6.4 Implement provider temporary-document publication/readback and saved receipts, including visible Saving/incomplete states and capability-specific guarantees; verify interrupted copy/rename, provider refusal, destination collisions, and restart never yield false completion or deletion of user files.
- [ ] 6.5 Finish source/destination pickers before entering exchange, scope keep-screen-on to active transfer, and retain real suspension cancellation; verify picker transitions, app backgrounding, later grant reopening, and foreground-only resume on an actual Android device.

## 7. Device Exchange user experience

- [x] 7.1 Add Vault data/Files & folders mode controls and Send/Receive preparation with Add files, Add folder, remove selections, paged tree preview, exclusions, and destination picker; verify supported desktop/phone layouts and preservation of the current vault approval/conflict experience.
- [x] 7.2 Connect typed transfer status/events to Preparing/Approval/Transferring/Verifying/Saving and show file counts, bytes, measured speed, supported ETA, and durable versus in-flight progress; verify large totals, unstable speed, and provider saving never display premature completion.
- [ ] 7.3 Add Pause/Stop, new-session Resume, partial/unconfirmed summaries, Open destination, and discard controls with translations; verify every resume requires a fresh code/approval and navigation/suspension closes authorization while retaining progress.
- [x] 7.4 Reuse network selection/manual hostname-IP-port entry and present LAN/VPN/firewall/DNS guidance plus provider constraints; verify MagicDNS/bracketed IPv6 entry, occupied ports, unavailable interfaces, unsupported iOS mode, and lack of automatic route/mode fallback.

## 8. Integration, performance, and release acceptance

- [x] 8.1 Complete current-library OPAQUE/channel-binding acceptance for the reused boundary, including wrong/expired/reused codes, parallel attempt limits, replay/downgrade/substituted channel, and approval rejection; verify no filename/offer/body disclosure precedes authentication and termination clears keys/sockets/listeners.
- [x] 8.2 Run scripted fault injection around file flush, journal commit, verification, publish, directory flush, receipt, and batch acknowledgement, including permissions/full disks/corruption/source edits; deliver results showing correct output, recoverable offsets, and confirmed versus unconfirmed outcomes.
- [ ] 8.3 Validate actual supported desktop peers and Android over LAN, cross-network Tailscale, and another routed private VPN with manual discovery fallback, IPv4/IPv6, denied access rules, and interruptions; record exact builds/platforms/routes and leave unavailable physical scenarios explicitly incomplete.
- [ ] 8.4 Build a same-encryption/storage/durability streaming benchmark and measure 4 GiB incompressible, 8 GiB boundary, and 10,000-file datasets on LAN and 20/80 ms RTT routed-VPN profiles; deliver phase timing/RSS/CPU/retransmission reports and tune bounded windows/checkpoint cadence to meet the 70 percent relative large-file target and below-64 MiB working-memory target.
- [ ] 8.5 Record real Tailscale direct and available relayed measurements separately and verify no runtime CLI dependency or unsupported fixed-speed claim; publish reproducible route/device/provider conditions alongside total transfer and saving time.
- [ ] 8.6 Run appropriate existing frontend/Rust/native vault regression checks and supported-platform builds after shared code changes; verify unchanged bidirectional vault behavior, Android native reproducibility, and file-mode compatibility before enabling the validated targets.
- [x] 8.7 Document file/folder workflow, native/provider constraints, fresh-pairing resume, storage/cleanup ownership, network troubleshooting, benchmark evidence, and rollback; verify all linked acceptance evidence exists and any unresolved target prevents an unsupported release claim.

## Implementation evidence (2026-10-06)

See [validation.md](validation.md) for executed checks, benchmark conditions, baseline regression failures, and remaining acceptance. Unchecked tasks may have implementation present; their required native/physical/network verification remains incomplete. File mode was enabled for release builds on Linux, Windows, and Android on 2026-10-07 after the user reported successful device file-transfer testing. Detailed acceptance tasks remain unchecked without their specific evidence.
