# Device file exchange implementation and validation

Recorded 2026-10-06. Implementation is present; **24/36 tasks are checked**. Remaining tasks retain their original acceptance requirements. Files & folders is enabled only when `debug_assertions` is true on Linux, Windows, or Android. Release file mode, macOS, and iOS remain disabled. Existing release Vault data exchange stays available.

The [workflow guide](../../../docs/device-file-exchange.md) describes selection, consent, saving, resume, cleanup, and troubleshooting. Non-UTF-8 desktop names currently stop preparation with an explicit error, rather than becoming a renamed exclusion; the delta specification permits explicit rejection. Supported unreadable/symlink/special entries become previewed exclusions. No silent filename sanitization or payload fallback is used.

## Environment and reproducibility

Host: Linux x86_64, Intel Core i9-14900K, Rust/Cargo 1.95.0, Node 22.22.1. Repository baseline: `7c25530`. The worktree implementation is identified by the source hashes in [checks.json](evidence/checks.json). Tests use owned temporary files and loopback peers, not user vaults or remotely discovered devices.

Android: Java 17, SDK/platform/build tools 36, NDK 28.2.13676358, minimum API 24. ARM64 and x86_64 debug APKs include the tracked Kotlin adapter and manifest. Build wrappers copy `src-tauri/android/app/src/main` into generated projects while preserving Tauri-generated bindings. The scaffold test verifies installation into a clean generated tree. No broad storage permission was added. No Android device or emulator was attached for provider/lifecycle acceptance.

Windows: the complete application cross-check uses `x86_64-pc-windows-gnu` with MinGW. Headless source tests run through Wine 10.0. This checks real Windows-compiled engine/publication code but does not replace Windows hardware/filesystem acceptance. Wine's `CreateSymbolicLinkW` returned success without creating a reparse entry; the strict native reparse test is preserved and explicitly filtered from Wine execution, rather than weakened to pass.

## Executed checks

| Check | Result |
| --- | --- |
| `npm test` | 9 frontend suites / 17 tests; 3 Node script tests pass |
| `npm run build` (also run by native build wrappers) | TypeScript and bundled production frontend pass |
| `cargo test --locked --lib file_exchange::` in `src-tauri` | 22 pass; 1 ignored child-process worker, invoked explicitly by its parent test |
| `cargo test --locked --lib sync::` | 27 pass, including legacy bidirectional vault/attachments and new authorization/watchdog checks |
| `cargo test --locked --lib -- --test-threads=4` | 260 pass; 5 baseline failures; 2 ignored workers/native-platform tests |
| Windows `cargo check --locked --target x86_64-pc-windows-gnu` | Complete application cross-check passes |
| Windows harness `cargo test --locked --manifest-path tools/file-exchange-harness/Cargo.toml --target x86_64-pc-windows-gnu --lib -- --skip windows_reparse` with Wine runner | 26 pass; 1 ignored child worker; 1 native reparse test filtered |
| `node scripts/android.mjs build --debug --apk --target aarch64` | ARM64 debug APK passes Rust, Kotlin, and packaging |
| `node scripts/android.mjs build --debug --apk --target x86_64` | x86_64 debug APK passes Rust, Kotlin, and packaging |
| Linux `npm run tauri -- build --debug --no-bundle` | Complete application builds successfully |

Windows commands require the MinGW C compiler, archiver, and linker configured for the target. Set `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine` for the Wine test command. The harness imports the actual repository authentication, manifest, journal, filesystem, and streaming engine source; it avoids WebView dependencies. Its lockfile pins the same cryptographic library versions as the application.

The full Rust suite's five failures were reproduced in a separate archive of unchanged `7c25530` (233 pass, 5 fail, 1 ignored):

- `commands::note_tasks::tests::links_retry_sync_in_both_directions_and_defer_locked_sources_without_content`: invalid/missing linked todo item.
- `notes::page_tree::tests::migration_preserves_hierarchy_ids_and_encrypted_data`: missing `title_is_encrypted` column.
- `release_tests::m3_upgrade_and_backup_restore_preserve_all_domains_and_ciphertext`: the same missing column.
- `notes::sections_crypto::tests::set_password_migrates_existing_pages_and_locks_search_out`: `QueryReturnedNoRows`.
- `notes::sections_crypto::tests::change_password_keeps_data_and_rotates_access`: `QueryReturnedNoRows`.

An initial heavily parallel build/test run additionally timed out the existing concurrent-space-write test. That test passed in isolation and in the final four-thread full run. These unrelated baseline failures remain unresolved and prevent an all-green regression claim. Summarized raw results are retained in [test-results.txt](evidence/test-results.txt).

## Stability, integrity, and authorization evidence

Actual TLS 1.3 / file-mode OPAQUE loopback sessions cover mixed Unicode paths, hidden/empty folders, zero-byte files, multi-chunk files, either listener role, delayed mutual consent, decline, edited sources before/during transfer, interrupted resume, corrupt retained chunks, and edited/missing completed outputs. Fresh pairing is required for every resumed round; repeat resume does not duplicate output.

Protocol tests cover vault/file binding separation and the unchanged legacy identifier, wrong codes, role/transcript/channel/key substitutions, conflicting roles/protocols before manifest disclosure, naming-plan/session approval binding, malformed/truncated/oversized frames, invalid entry/range identifiers, tiny data chunks, and empty-window flooding. Session tests cover a single winner among 16 parallel pairing attempts, global failure limits, expired/single-use codes, socket shutdown on Stop, and distinct approval/progress watchdogs. An actual relay terminates TLS on two connections and forwards the greeting and OPAQUE messages unchanged. Both endpoints reject it before finalization or content, including when the relay reuses the exact certificate/key to isolate TLS-exporter binding. This checks the pinned library composition; it does not claim a new independent cryptographic audit.

Fault injection covers write permission failure, low-space reporting, chunk corruption, source edits, and interrupted publication. Eleven abrupt child-process exits terminate without Rust destructors at write, file flush, journal commit, verification, ready record, publication rename, directory flush, publish return, receipt commit, receipt send, and batch acknowledgement. A fresh process and fresh authenticated session recover the owned output without duplicates or falsely confirmed sender success. Injection uses simulated write/space errors; it does not establish every real filesystem's ENOSPC or power-loss behavior.

Selection tests cover overlap deduplication (including child-first native-root selection), portable reserved names, Unicode/case collisions, budgets, checked totals, and missing parent rows. Owned-discard tests preserve source files, completed files, and unowned staging entries. Android provider tests requiring actual grants/descriptors remain open.

## Local throughput and memory evidence

Run the benchmark from the repository root:

```bash
node scripts/benchmark-device-files.mjs --dataset large4 --storage-dir /tmp --out artifacts/file-exchange/large4.json
node scripts/benchmark-device-files.mjs --dataset large8 --storage-dir /tmp --out artifacts/file-exchange/large8.json
node scripts/benchmark-device-files.mjs --dataset small --storage-dir /tmp --out artifacts/file-exchange/small.json
```

The script builds a locked release harness and generates owned deterministic pseudorandom data in a temporary directory under the requested filesystem. It runs the baseline and file engine sequentially in independent processes and cleans only its own temporary dataset. The single-file baseline uses the same TLS/OPAQUE versions, chunk/whole hashes, checkpoint cadence, durable journal, no-replace publication, and window bounds. Ten thousand 4 KiB files are measured as a batch; no equivalent raw single-file baseline ratio is asserted for that dataset.

**Conditions:** both peers in one native process on IPv4 loopback; `/tmp` is tmpfs; no RTT/loss shaping; no Android provider or WebView. The workstation was not reserved for benchmarking, so scheduling noise may affect timings. RSS is sampled every 10 ms and reports incremental process RSS for both peers combined after preparation, excluding dataset generation/scan. Phase timings, preparation/generation, CPU seconds, exact bytes, filesystem type, and confirmed outcomes are recorded in each JSON file. Page-cache/dataset storage is not counted as transfer working memory. Network retransmission counters were not measured on loopback.

| Dataset | Session seconds | Payload rate relative to baseline | Additional RSS | Evidence |
| --- | ---: | ---: | ---: | --- |
| 4 GiB pseudorandom file | 4.973 | 99.5% | 22.2 MiB | [large4](evidence/loopback-large4.json) |
| 8 GiB boundary file | 9.679 | 99.5% | 23.3 MiB | [large8](evidence/loopback-large8.json) |
| 10,000 × 4 KiB files | 1.048 | Not compared | 24.0 MiB | [small](evidence/loopback-small.json) |

These local samples satisfy the relative-throughput and working-memory thresholds under the stated conditions only. They do not establish the 70% target at 20/80 ms RTT, real LAN disk performance, Tailscale direct/relay throughput, Android saving time, or a fixed speed guarantee. Tasks 8.4 and 8.5 remain open.

## Interface evidence

The real file panel and application CSS were rendered with bounded mocked native metadata in Chrome's viewport emulation. [Layout results](evidence/ui-layout.json), [390 px phone screenshot](evidence/ui-phone.png), and [1280 px desktop screenshot](evidence/ui-desktop.png) show visible consent and no horizontal overflow. These are component layout checks, not physical Android screenshots or lifecycle acceptance. Frontend tests also distinguish Saving at 100% from confirmed completion, partial/unconfirmed outcomes, bilingual approval, and exact large totals.

## Remaining acceptance and release gate

| Scenario | Status / required evidence |
| --- | --- |
| Native Windows destination/reparse/UNC races and filesystem durability | Pending real Windows runner; Wine filter is explicit |
| Android local and third-party document providers | Pending real grants, unknown-length/non-seekable/large documents, revoked grants, space exhaustion, interrupted Saving/readback, restart, collision, and owned cleanup tests |
| Android lifecycle and foreground resume | Pending actual picker transitions, onPause/backgrounding, screen-on scope, resumed grants, and cross-device transfer |
| Linux ↔ Windows ↔ Android LAN | Pending physical peer builds, exact routes, IPv4/IPv6, firewall denials, and interruptions |
| Cross-network Tailscale | Pending separate direct and available relayed measurements, MagicDNS/manual entry and access-rule denial |
| Another routed private VPN | Pending manual discovery fallback, policy denials, and interrupted resume |
| 20/80 ms RTT profiles | Pending shaped LAN/VPN throughput, RSS/CPU/retransmission and checkpoint tuning |
| Complete vault regression acceptance | Five demonstrated baseline failures plus physical platform checks remain open |

No connected Android peer, authorized remote recipient, or native Windows runner was available in this workspace. Reachable local LAN/Tailscale interfaces alone are not peer-transfer evidence. No runtime Tailscale CLI dependency was introduced. The work leaves file mode gated rather than advertising unvalidated targets as release-supported. Do not archive the change or check the remaining tasks until their evidence exists.
