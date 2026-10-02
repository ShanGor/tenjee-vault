# Tasks

## 1. Mobile platform foundation

- [ ] 1.1 Gate desktop plugins, tray/window-close behavior, global shortcuts, and window-state setup in Cargo/lib.rs; verify desktop builds and Android/iOS target compilation on their supported runners.
- [ ] 1.2 Generate Android/iOS targets and configure sandbox storage and local-network permissions; verify installation and first-run offline initialization on one real device per platform.
- [ ] 1.3 Add mobile document picker/share and clipboard adapters; verify import, export, attachment opening, cancellation, and denied-permission behavior on both mobile platforms.
- [ ] 1.4 Add OS-scheduled mobile reminders and lifecycle reconciliation; verify task/calendar reminders while suspended and clear handling of denied notification permission.

## 2. Adaptive user experience

- [ ] 2.1 Implement compact shell navigation, More, search, quick capture, safe areas, touch sizing, and keyboard handling; verify 320/390/600/768/1100 CSS px layouts, rotation, enlarged text, and no module-level horizontal overflow.
- [ ] 2.2 Implement notes drawer, read/edit toolbar, breadcrumbs, save state, and explicit move/reorder controls; verify nested protected pages, mobile editing with keyboard open, table scrolling, and save failures before navigation.
- [ ] 2.3 Implement task selectors, full-screen detail/creation, explicit selection/status controls, and one-column Kanban; verify hierarchy, medication course forms, and returning to the prior list position.
- [ ] 2.4 Implement phone Agenda default, compact month picker/day list, Day timeline, readable Week, and full-screen event forms; verify busy days, lunar information, and separate compact/wide view preferences.
- [ ] 2.5 Adapt Tags and Settings for single-column mobile forms; verify all actions by touch and Chinese/English labels at narrow widths.

## 3. Replication storage

- [ ] 3.1 Add replica identity, causal revisions, journals, tombstones, conflict storage, and checkpoints to owning database migrations; verify upgrade fixtures preserve IDs, ciphertext, wrapped keys, and legacy internal sentinel records.
- [ ] 3.2 Integrate ordinary notes/page-tree/template/tag writers and attachment metadata with atomic revision writes; verify fault injection cannot leave committed data without a revision.
- [ ] 3.3 Integrate task/list/bulk/archive/recurrence/course and calendar/event/exception/tag writers; verify generated doses and recurrence changes each replicate with stable identities and transactional journals.
- [ ] 3.4 Integrate protection/domain membership/password-wrapping operations as coherent dependency groups; verify interrupted transitions preserve decryptable versions and no plaintext staging leak.
- [ ] 3.5 Implement restore/clone identity rotation and revision baseline handling; verify two restores of one backup produce concurrent distinct origins and reset peer reconciliation cursors.

## 4. Discovery and authentication

- [ ] 4.1 Implement explicit local-interface discovery/advertising/listeners and manual IP/port fallback; verify no sockets/advertisements outside mode, local IPv4/IPv6 connection, and public-address rejection.
- [ ] 4.2 Select and pin established PAKE/TLS implementations and document the complete channel-binding construction; verify published protocol vectors and a reviewed authentication/downgrade/replay threat model before integrating payload transfer.
- [ ] 4.3 Implement eight-digit code expiry, global five-attempt limit, single-use consumption, and mutual key confirmation; verify wrong/expired/replayed codes, parallel attempts, and intercepted handshakes cannot gain data access.
- [ ] 4.4 Implement bounded protocol/schema negotiation, authenticated summary, and mutual scope approval; verify rejection, incompatible versions, or missing approval prevents workspace payload transfer.
- [ ] 4.5 Implement session stop/finish/inactivity/suspension teardown and secret/log hygiene; verify every termination path closes listeners, clears volatile secrets, and requires fresh pairing.

## 5. Replication engine and conflict handling

- [ ] 5.1 Implement typed entity inventory/envelopes and delta requests with explicit device-local exclusions; verify full-workspace round-trip covers history, templates, tags, medication tasks, event exceptions, and links while retaining local settings/paths.
- [ ] 5.2 Implement causal successor/concurrency detection, deterministic conflict variants, and deletion propagation; verify clock skew, edit/edit, edit/delete, long-offline deletion, and repeated delivery converge without silent data loss.
- [ ] 5.3 Implement validated hierarchy/move resolution and select/keep-both conflict actions; verify cycles and protection nesting are rejected and resolution revisions converge across three replicas.
- [ ] 5.4 Implement ciphertext-only protected replication and coherent key-change conflicts, including deferred protection/plaintext races; verify locked-to-locked exchange, unlocked source with locked destination, and password changes without key leakage.
- [ ] 5.5 Integrate replicated task-to-note checkbox changes with existing local queues using trigger provenance; verify locked/missing-page recovery and A↔B↔C exchanges do not echo updates or duplicate revisions.

## 6. Transfer recovery and exchange screens

- [ ] 6.1 Implement bounded chunked blobs, manifests, hash/free-space/path validation, and promotion before references; verify oversized/malformed/corrupt/path-traversal payload rejection and large-attachment transfer.
- [ ] 6.2 Implement idempotent staged multi-database application and acknowledgment only after durable commit; verify crashes between domain commits, cancellation, disk exhaustion, and fresh-pairing resume preserve committed data without duplicate effects.
- [ ] 6.3 Add localized desktop/mobile discovery, code, preview/approval, progress, conflict, and result screens; verify actual phone/laptop exchange includes actionable permission/isolation errors and accurately reports partial/pending work.

## 7. Release acceptance

- [ ] 7.1 Run real Android↔laptop and iOS↔laptop exchanges plus laptop↔laptop/three-replica fixtures; record initial sync, offline changes both ways, deletion, conflicts, protected data, interruption, and repeat-sync outcomes.
- [ ] 7.2 Complete frontend/Rust checks and desktop install/offline/backup regression checks; record mobile build artifacts, platform/OS coverage, and limitations in a validation document.
- [ ] 7.3 Update release documentation and supported-platform claims only after acceptance; verify the documented pairing flow, compatibility policy, and recovery limitations match shipped behavior.
