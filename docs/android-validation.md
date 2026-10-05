# Deferred Android phone validation

Use the ARM64 debug APK from [Android development](android-development.md) and a laptop running the same checkout with `npm run tauri -- dev`. Use disposable workspaces and backups while evaluating development-only LAN exchange. No checklist item below has been physically validated in this workspace.

## Install and mobile editing

- Install/upgrade without clearing app data; start offline and confirm prior notes, tasks, calendar entries, and attachments remain available.
- Check Notes/Tasks/Calendar/More at narrow width, rotation, enlarged text, gesture navigation, and with the keyboard open. Verify Android Back dismisses sheets, saves edits, and reports save failures.
- Protect a tree, unlock and edit titles/body, lock it again, and confirm neutral titles. Open a vault with legacy protected titles, unlock to migrate them, then confirm backup/restart preserve decryptability.

## Native files and backups

- Import Markdown/text/calendar documents from local and cloud providers; cancel selection and save dialogs. Export notes, subtree ZIP, calendar, and `.tvault` backups to a system folder.
- Open/share an ordinary attachment and a protected attachment. Confirm protected sharing requests plaintext consent and cancellation creates no share. Check another app can read the granted file and stale temporary files are cleaned after their retention period.
- Choose an automatic backup folder, create several backups, confirm owned-file retention leaves unrelated files untouched, and restore a backup. Revoke provider access or interrupt a copy and check the error/retry behavior.

## Background reminders

- Grant notifications, schedule task/calendar reminders, background the app, and check delivery. Repeat with notifications denied, exact-alarm access denied/granted, a locked screen, reboot, and timezone/clock changes.
- Reopen after delivery and confirm no duplicate. Edit/delete reminders and confirm old alarms stop. Confirm sync does not transfer delivered history or permissions and incoming definitions schedule locally.
- Check Settings' scheduling horizon and the 256-alarm limit, plus Android battery controls/force-stop behavior. Delivery times may be approximate without exact access.

## Phone and laptop exchange

1. Join a reachable non-isolated LAN, or a private VPN/Tailscale network that allows client connections. Open More → Device exchange on the phone and Device exchange on the laptop. Keep Automatic LAN selection for LAN use; for VPN use, refresh and select the appropriate interface on both devices. Start mode on both.
2. Select a receiver or enter its hostname/IP and displayed port. Test short/full MagicDNS names on Tailscale, regular VPN hostnames and routed private IPs, IPv4/IPv6, and numeric-IP fallback when DNS fails. Enter its eight-digit code including leading zeros. Check wrong/expired codes fail. Confirm both devices must approve full-workspace scope before data transfer.
3. Exchange independently created notes, children, history, templates, tasks/lists/medication doses, linked checkboxes, recurring calendar exceptions, tags, and attachments. Confirm settings and backup paths remain local.
4. Edit different items offline on both and exchange again. Repeat without new changes; check no duplicates. Delete offline and check deletion propagation.
5. Edit the same item on both. Review both variants, choose one, then exchange again. Repeat with keep-both for page/task subtrees and verify internal references and inbound-link behavior.
6. Exchange locked protected trees and attachments, unlock locally using their protection passwords, and verify titles/body/history. Race a password/protection change against an offline edit; check coherent alternatives or pending status without plaintext fallback.
7. Toggle a linked task while its note is missing/locked on the other device. Exchange the page/unlock it later and verify the checkbox catches up without repeated echoed edits.
8. Stop, leave the screen, lock the phone, disconnect Wi-Fi, or interrupt a large attachment transfer. Reopen, pair with a fresh code, and check durable changes/resume behavior. Clear pending files and confirm committed data/source edits remain.
9. Try discovery blocked by firewall/guest isolation; use manual address fallback on an eligible local subnet. Confirm public addresses are rejected and no connection is accepted outside active mode.
10. Repeat across different physical networks using Tailscale and a regular VPN. Verify client isolation/access-control failures give guidance, VPN discovery is not required, and only the selected interface addresses listen. Test a fixed port across sessions/restart, an occupied port, automatic ports, mixed DNS answers, and Stop during DNS/connection attempts. Record Android VPN interface and MagicDNS behavior; these scenarios require physical validation.

Record device/Android/WebView versions, build identifiers, provider/router details, observed outcomes, and failures. This checklist complements the outstanding automated protocol, malformed-input, crash-recovery, and three-replica acceptance requirements; it does not replace them.
