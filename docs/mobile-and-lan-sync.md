# Mobile experience and local network exchange

Status: Device Exchange enabled in Debug and release builds, 2026-10-05, following user-reported successful testing. Adaptive layouts and Android native files/reminders are implemented; see [Android build and install instructions](android-development.md). Broader mobile acceptance and iOS implementation remain in progress. See the [implementation status](../openspec/changes/mobile-and-lan-sync/implementation-notes.md) and [pairing implementation notes](lan-pairing-implementation.md). The normative requirements and implementation plan are in [the mobile-and-lan-sync change](../openspec/changes/mobile-and-lan-sync/proposal.md), its [specifications](../openspec/changes/mobile-and-lan-sync/specs/), [design](../openspec/changes/mobile-and-lan-sync/design.md), and [tasks](../openspec/changes/mobile-and-lan-sync/tasks.md).

## Product direction

Tenjee Vault remains a local-first personal workspace. Android and iOS will use the existing React interface and Rust data layer through Tauri mobile targets. Each device has its own local vault and works offline. Users can intentionally exchange changes between their own devices on a reachable LAN without an account, a Tenjee server, or cloud storage.

## Mobile screens

| Area | Phone experience |
| --- | --- |
| Navigation | Bottom tabs: Notes, Tasks, Calendar, More. Search and quick capture remain accessible. More contains Tags, Settings, and device exchange. |
| Notes | Full-width reading/editing; space selector and expandable page tree in a drawer; compact breadcrumbs and contextual formatting; explicit move/reorder commands. |
| Tasks | List by default; compact list/smart-filter selector; full-screen detail and creation forms; optional Kanban with one status column at a time. |
| Calendar | Agenda on first use; compact month date picker above a selected-day event/task list; Day timeline and readable Week summaries; secondary actions in menus. |
| Forms | One column, touch-sized controls, keyboard-aware focused fields and completion actions. Long forms use full-screen views. |
| Protected pages | Existing read-first and password protection behavior, including inherited protection of child pages. Pairing does not unlock pages. |

Phone layouts target 320–600 CSS px, tablet layouts 601–900, and wide layouts above 900. Layout follows available width; OS-specific features follow platform capabilities. Core screens must not require sideways scrolling. Wide document tables can scroll inside their own region. Respect safe areas, larger text, and rotation; use at least 44 × 44 px primary touch controls and 16 px input text. File import/export and notifications use mobile integrations and explain permission limits. Mobile reminders use OS scheduling; exchange runs while the app is active rather than relying on background execution.

## Find other devices and exchange

1. Connect the phone and laptop to a reachable LAN or a VPN that permits device-to-device connections, and open **Find other devices and exchange** on both. On a phone, find it under **More**; on desktop, use **Device exchange** in navigation or Settings. Both devices must run compatible builds; Debug and release builds support exchange.
2. Keep **Automatic LAN selection** for nearby devices, or refresh the network list and select the appropriate VPN/Tailscale interface on each device. Optionally set an **Exchange port** from 1–65535 for a stable listening port; leave it blank for automatic allocation. The fixed port is saved only on this device, and an occupied port reports an error. Choose **Enter exchange mode** on both devices. Select one device as the receiver; it displays its listening addresses and a temporary eight-digit code, including any leading zeros. LAN sessions advertise locally; VPN sessions use manual entry.
3. On the other device, select the receiver from nearby devices or enter its hostname/IP and listening port, then enter the code shown on its screen. Examples: `my-laptop:49152`, `my-laptop.tail1234.ts.net:49152`, `192.168.1.20:49152`, or `[fd7a:115c:a1e0::2]:49152`. Use the port displayed by the receiver; devices can use different ports. Hostnames use system DNS, including Tailscale MagicDNS when configured. LAN `.local` names require OS resolver support; numeric IP entry remains available. Link-local IPv6 uses `[fe80::2%interface-index]:port` with the connecting device's interface index. URLs are not accepted.
4. Both devices show the authenticated peer, full-workspace exchange scope, and change/conflict summary. Approve on both to begin.
5. Changes transfer in both directions. Progress shows records and attachments; the result distinguishes applied changes, conflicts, pending changes, and failures.
6. Review conflicts immediately or later. Choose a version or keep both. The resolution reaches other devices during the next exchange.
7. Completion or Stop closes the network session. A later exchange starts with a new code.

```mermaid
sequenceDiagram
    participant Phone
    participant Laptop
    Note over Phone,Laptop: Both explicitly enter exchange mode
    Laptop-->>Phone: Local discovery advertisement
    Note over Laptop: Display temporary authentication code
    Note over Phone: User enters laptop's code
    Phone->>Laptop: Password-authenticated pairing
    Laptop->>Phone: Mutual authentication confirmation
    Phone->>Laptop: Approve workspace exchange
    Laptop->>Phone: Approve workspace exchange
    Phone->>Laptop: Missing revisions and attachments
    Laptop->>Phone: Missing revisions and attachments
    Note over Phone,Laptop: Show results and conflicts; close session
```

The code is valid for five minutes, single use, and invalidated after five failed authentication attempts in total. It authenticates a secure session rather than acting as a note password. Codes are not sent as plaintext, advertised, logged, or saved. A password-authenticated key exchange protects short-code pairing against passive offline guessing; the protocol choice and transport binding require implementation review. The implementation uses OPAQUE over TLS 1.3; see [the composition and outstanding acceptance](lan-pairing-implementation.md).

No device is discoverable or accepts sync connections outside exchange mode. Stop, completion, app closure/suspension, or 30 minutes without transfer/user activity ends authorization. Sync history is retained for efficient deltas, but does not grant permission to reconnect automatically. Network access uses selected LAN/VPN addresses. The app supplies no cloud relay or router port mapping; Tailscale or another user-managed VPN may route or relay its own encrypted traffic. Public Internet endpoints remain unsupported. Being on the same router or VPN is insufficient if guest/client isolation, firewall rules, or VPN access controls block peers. DNS errors, invalid address results, unavailable ports, and blocked connections provide guidance. Never weaken authentication to work around discovery failure.

## What is exchanged

The initial release synchronizes the complete workspace: spaces/page trees, note content and history, applicable templates, protected-domain metadata, tasks and lists, medication dose tasks, linked checkboxes, calendar events/exceptions/reminder definitions, tags, associations, and attachments. Existing IDs and cross-module links remain stable. Initial exchange inventories all content; later exchanges send only missing changes and attachment blobs. Independently created content on both devices is retained, including distinct default lists/spaces.

Device settings stay local: appearance/navigation state, filesystem paths, permissions, delivered-notification history, pending local work queues, backup directories, exchange port preference, and unlocked sessions are excluded. Selective folder sharing, public Internet/cloud sync, permanent trust, and continuous background sync are outside this first release.

## Offline edits, conflicts, and failures

Changes to different objects merge automatically. Concurrent changes to the same object preserve both variants and appear as conflicts; a newer wall-clock timestamp never silently wins. Delete-versus-edit conflicts retain both the deletion and edited content for a decision. Ordinary causally newer deletions propagate, and deletion records are retained so an old offline device cannot resurrect unchanged content. Moves and protection changes must also respect tree validity and domain boundaries.

Protected content moves as stored ciphertext plus its wrapped key metadata, even if the source is unlocked. The receiving device remains locked until the user enters the protection password locally. Concurrent key/protection changes retain coherent versions. A protection change racing with a previously ordinary edit may pause that group until local unlock allows the conflict to be encrypted safely; original revisions stay on their sources. Pairing never transfers unwrapped keys, note passwords, or decrypted search indexes.

Transfers validate schemas, bounds, hashes, references, and free disk space. No visible attachment reference points to a partially received blob. Wi-Fi loss, suspension, cancellation, or a crash preserves valid committed work and recoverable progress; reconnect with a fresh code to resume. Stopping does not undo already committed changes. An interrupted exchange reports a partial result rather than success. Changes made after its approved snapshot are shown as pending for the next exchange.

Sync merges logical objects; it does not replace database files or run backup restore. Keep backups as a separate recovery mechanism. Restoring or cloning a vault generates a new local replica identity while retaining object IDs/history, then reconciles peers again so revision counters do not collide. Incompatible protocol/schema versions stop before workspace writes and explain which app needs updating.

## Delivery order and acceptance

Deliver the adaptive UI and mobile platform integration, then durable change tracking and authenticated LAN exchange, then validate recovery and conflicts before release. Cover both Android and iOS with real phone/laptop exchanges, desktop regressions, offline editing, repeated sync, protected trees, concurrent edits/deletes/moves, linked checkboxes, large attachments, network interruption, insufficient space, denied permissions, clock skew, and backup restoration. Also check A↔B↔C propagation so history remains correct when a device exchanges with more than one peer.

Tauri supports Android/iOS in addition to desktop; mobile plugin support must be checked individually. iOS builds require a macOS/Xcode environment. [Tauri platform overview](https://v2.tauri.app/start/), [plugin support](https://v2.tauri.app/plugin/), [mobile prerequisites](https://v2.tauri.app/start/prerequisites/).
