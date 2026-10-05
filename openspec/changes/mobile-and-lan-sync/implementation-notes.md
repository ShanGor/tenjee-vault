# Implementation status — 2026-10-03

Android development implementation is available. This is not a mobile release: physical phone validation is deferred at the user's request, iOS is not implemented, and protocol acceptance is outstanding. The OpenSpec tasks bundle implementation with acceptance, so all 31 checkboxes remain open.

## Delivered code

- Adaptive Notes/Tasks/Calendar/More shell, compact drawers/forms, phone Agenda, explicit hierarchy actions, safe-area/IME handling, save-aware Android Back, and desktop plugin gating.
- Android native document import/save/share, temporary FileProvider grants, protected plaintext consent, system backup folder authorization, verified backup publishing and owned-file retention.
- Durable AlarmManager task/calendar scheduling, exact/inexact fallback, permission status, boot/time-change reinstall, fired-history reconciliation, and bounded scheduling windows.
- SQLite revision triggers/journals/tombstones, replica rotation, causal conflicts, coherent protection groups identified by root page, and device-local exclusions.
- Foreground local discovery/listeners, manual same-subnet address fallback, expiring globally limited single-use codes, TLS 1.3/OPAQUE channel binding, authenticated schema negotiation, and mutual full-workspace approval.
- Snapshot/delta exchange, bounded/resumable blob transfer, integrity/free-space validation, durable recovery manifests and per-store receipts, progress/results, conflict selection and keep-both subtree duplication.
- Protected titles encrypted at creation/save and migrated on local unlock; neutral locked labels and eligibility checks keep legacy plaintext out of protected exchange. This refinement was approved by the user.
- Complete ciphertext alternatives preserve key-change conflicts. Competing ordinary edits remain pending when locked, or are encrypted in memory with a local unlocked key before staging. Linked task checkbox changes use local projections/provenance without generating echoed note-body revisions; missing or locked source pages remain pending.

## Current build checks

`npm run build` and Linux `cargo check` passed after native integrations and the final title/queue/key-invalidation changes. Both bundled Android debug APKs built successfully for ARM64 and x86_64 after the final source changes. `git diff --check` passed. Vite's large-chunk warning and Gradle's future-deprecation warning remain.

No new automated tests were added or run during this continuation. Previous observations below are historical and do not validate the current native integrations, OPAQUE composition, replication, or protection migrations.

## Checks already performed

| Check | Recorded outcome | Limit |
| --- | --- | --- |
| `npm run build` | Passed after the frontend edits | Vite reports a bundle-size warning; this is not a native mobile build |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed with OPAQUE and replication storage | Linux desktop only |
| Browser layout smoke checks | Passed at 320, 390, 600, 768, and 1100 CSS px across Notes, Tasks, Calendar, Tags, Settings, and More | Mock native commands; before the final task-parent/list controls and note navigation error changes |
| Drawer/task/calendar browser interactions | Drawer Escape dismissal, full-width task detail, compact month and event form checked | No phone keyboard, real database, or native document picker |
| Initial storage unit checks | 3 passed | Before incoming application and later validation changes |
| Full Rust library run | 214 passed, 4 failed, 1 ignored | Four migration fixtures expected old latest versions; their migration slices were corrected, but the full suite has not been rerun |
| Pairing/storage prototype run | 6 passed | This run used the earlier SPAKE2 prototype; **it does not validate the current OPAQUE implementation** |


## Acceptance still required

- Physical Android file-provider cancellation/sharing/backup behavior, suspended reminders, permission denial, keyboard/rotation/accessibility, and phone↔laptop exchange. The user will perform phone validation later; see [the checklist](../../../docs/android-validation.md).
- Current OPAQUE vectors, wrong/expired/replayed codes, channel substitution/downgrade, parallel attempt limits, teardown, malformed peer input, and a composition review. LAN remains development-only until these are accepted.
- Migration/regression fixtures, protection/password/move races, interruption/disk-exhaustion recovery, attachment bounds, repeated exchanges, and three-replica convergence. Historical suites are not acceptance of this code.
- iOS native target and integrations, build/device acceptance on a Mac/Xcode runner, and desktop install/offline/backup regression acceptance.

No acceptance task is marked complete based only on compilation. See [tasks](tasks.md), [Android development](../../../docs/android-development.md), and [LAN construction](../../../docs/lan-pairing-implementation.md).

## Current APK artifacts

| Architecture | File | Bytes | SHA-256 |
| --- | --- | --- | --- |
| ARM64 phone | `artifacts/android/tenjee-vault-debug-aarch64.apk` | 63,969,640 | `00c3ab6ed39f34095d567fbbd9504a4023c09dfd37dfb6a5ab98a61d4a532e32` |
| x86_64 emulator | `artifacts/android/tenjee-vault-debug-x86_64.apk` | 70,537,437 | `7024741fb83b296abd1bc14e94936ab64e0b38ece218cdc56d86ef71b2fc4230` |

These debug builds contain bundled frontend assets and permit development-only LAN exchange. Installation and current native flows have not been physically validated.

## Hostname and VPN extension — 2026-10-05

Device exchange now accepts hostname/IP and port entry through the OS DNS/MagicDNS resolver. Automatic LAN selection retains same-link checks; explicit interface selection supports routed private/VPN peers and Tailscale IPv4/IPv6 addresses. Listeners and outbound source addresses use the selected interface addresses. VPN interfaces do not use mDNS. A locally saved fixed listening port is optional; blank keeps automatic allocation, and occupied fixed ports report an error. DNS/result limits, cancellation checks, address-family fallback, IPv6 scopes, and public/special-address rejection are enforced before pairing. The existing authentication, mutual approval, and release gate remain in place.

Recorded checks for this extension: 11 targeted Rust network tests passed (including actual loopback TCP source binding, connection fallback/cancellation, occupied-port handling, and listener release); Linux Rust library checking and ARM64 Android Rust library checking passed; the frontend production build and frontend/script tests passed; strict OpenSpec validation and diff whitespace checks passed. Dependency verification uses the repository's npm lockfile. No frontend dependency changes are included.

Live LAN/VPN/Tailscale exchanges, Android interface visibility and MagicDNS behavior, and full stop/suspension acceptance remain unverified. The updated [phone checklist](../../../docs/android-validation.md) covers these scenarios. Existing copied APKs above predate this extension and were not rebuilt. Task 4.1 includes physical networking acceptance and remains open; these checks do not complete the change's broader protocol/mobile/release acceptance.
