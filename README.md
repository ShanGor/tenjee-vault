# Tenjee Vault

Tenjee Vault is a private, local-first desktop workspace for notes, tasks, and calendar events.
Your data stays on the device: it is not synced to a Tenjee-operated service and the application
works offline after installation.

## Supported platforms and installation

Tenjee Vault 1.0.0 provides no-install, portable executables for:

- Windows 10/11: standalone `tenjee-vault.exe`
- macOS: `.app` or `.dmg`
- Linux: AppImage (a single executable — `chmod +x` and run, no installation)

Download the build matching your operating system from the release page, then launch
**Tenjee Vault**. The Windows executable requires the WebView2 runtime, which is preinstalled on
Windows 10/11. The first launch creates an empty local workspace and a default task inbox.
Development builds are not required at runtime.

To build locally, install Node.js 22+ and stable Rust, then run:

```bash
npm ci
npm run tauri build
```

To run the desktop development app, use `npm run tauri -- dev`. To build a standalone
Debug executable, use `npm run tauri -- build --debug --no-bundle`; on Linux the executable
is `src-tauri/target/debug/tenjee-vault`, and on Windows it is
`src-tauri/target/debug/tenjee-vault.exe`. **Device exchange** is available in both
Debug and release builds through the top navigation.

Before launching a different build, fully **Quit** any running Tenjee Vault instance,
including one hidden in the system tray. Desktop builds share a single-instance identity:
launching a second executable focuses the existing process and exits, even if the new
executable was just rebuilt. Rebuilding does not update an already-running app.
On Linux and Windows, a launch from a terminal reports the existing process ID when this
happens. On Windows, check running builds in PowerShell with
`Get-Process tenjee-vault -ErrorAction SilentlyContinue | Select-Object Id, StartTime, Path`.

Each platform builds only its own target, declared in the per-OS configs:
[src-tauri/tauri.windows.conf.json](src-tauri/tauri.windows.conf.json) (bundling disabled; the
portable exe is at `src-tauri/target/release/tenjee-vault.exe`),
[src-tauri/tauri.macos.conf.json](src-tauri/tauri.macos.conf.json) (`.app` + `.dmg`), and
[src-tauri/tauri.linux.conf.json](src-tauri/tauri.linux.conf.json) (AppImage). On Linux, if FUSE is
unavailable, run the build with `APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri build`.

## Data location and uninstalling

The application data directory is named `tenjee-vault` within the operating system's application
data location:

| Platform | Usual location |
| --- | --- |
| Windows | `%APPDATA%\\com.sam.tenjee-vault\\tenjee-vault` |
| macOS | `~/Library/Application Support/com.sam.tenjee-vault/tenjee-vault` |
| Linux | `$XDG_DATA_HOME/com.sam.tenjee-vault/tenjee-vault` (usually `~/.local/share/...`) |

Uninstalling the application package normally preserves this directory. This allows an upgrade or
reinstall to reuse your notes, tasks, calendar, and preferences. To remove all data, first create
and verify a backup, uninstall the app, then manually delete the data directory. For sensitive
storage, use your operating system or encrypted-disk tool's secure-delete guidance; ordinary file
deletion is not guaranteed to erase old data on SSDs or copy-on-write filesystems.

## Backups and recovery

Use **Settings → 立即备份…** to choose a `.tvault` destination. Tenjee Vault snapshots all
databases and attachments consistently, writes the archive atomically, then verifies its manifest
and SHA-256 hashes before reporting success. The application can also create daily or weekly
automatic backups, retain a configured number of managed automatic backups, and leaves manual
backups untouched.

To restore, use **Settings → 选择备份并恢复…**. The app first shows a preflight summary and checks
the archive structure, hashes, database integrity, and schema compatibility. After you confirm,
it locks encrypted sessions and asks you to restart. At next startup the restored data directory
is atomically installed; the previous one is kept as a managed recovery copy. If migration or
integrity checking fails, the original directory is restored and Settings displays the diagnostic.
Use **清理恢复前副本** only after confirming the restored data is correct.

Keep at least one verified backup on separate storage before an operating-system reinstall or a
destructive restore. A backup contains protected page trees as ciphertext; restoring it does not
weaken their password protection.

## Organizing notes

Each space contains an expandable tree of pages. Every page can hold content and child pages.
Use **New page** below the page list to create a root page, or **New child page** in a page's menu.
Drag onto a page to make it a child; drag between rows to reorder it. Existing notebooks, groups,
and sections become editable parent pages on upgrade, preserving existing page links and data.

Protection is inherited by child pages. A protected tree keeps its password when moved beneath
an ordinary page. Independent protected trees cannot be nested; remove one protection first.

Pages open in read-only mode. Choose **Edit** to change the title or content, then
**Done editing** to save and return to reading. Opening another page starts in read-only mode.

## Encryption and password recovery

A page and its children can be protected with the password you set. Tenjee Vault does not upload,
store, or provide a recovery copy of that password. If it is forgotten, the encrypted content
cannot be recovered. Exporting an unlocked encrypted page is explicitly confirmed because the
chosen output becomes plaintext.

## Import, export, and known limits

Notes can import UTF-8 Markdown, HTML, and plain text; pages and page trees export to Markdown,
HTML, or PDF and can use the print view. Calendar import/export uses UTF-8 iCalendar (`.ics`);
see [the iCalendar guide](docs/calendar-ical.md) for recurrence and timezone details.

v1.0 intentionally does not include cloud sync, collaboration, mobile apps, automatic updates,
package signing/notarization, OneNote `.one` import, audio recording, or a freeform canvas. HTML
import accepts only a safe supported subset, and lunar recurring events must be exported over a
finite date range as Gregorian instances.

## Device exchange and mobile development

Current source builds include an explicit **Find other devices and
exchange** mode in both Debug and release builds for devices on a reachable LAN or private VPN, including Tailscale. Manual
hostname/IP and port entry, network selection, and an optional fixed listening port support VPN use. Devices pair with a
random, temporary authentication code, approve on both screens, and synchronize notes, tasks,
calendar, tags, and attachments in both directions over an authenticated encrypted connection.
Offline edits and conflicts are preserved; protected page trees remain encrypted and still require
their own passwords. No account or hosted sync service is required.

Device Exchange was enabled for release builds after the user reported successful testing on 2026-10-05.
See [mobile and LAN exchange](docs/mobile-and-lan-sync.md)
for the mobile layouts, pairing workflow, scope, conflict rules, and recovery behavior, and the
[OpenSpec proposal](openspec/changes/mobile-and-lan-sync/proposal.md) for implementation planning.

Android development now has a native project and build commands. See
[Android development](docs/android-development.md) for APK builds, device installation, and the
native file sharing, scheduled reminders, LAN pairing, and the deferred phone validation checklist. This development target is not part of the v1.0 support claim; iOS implementation remains pending.

## v1.0 changes

- Portable note and calendar import/export, printing, verified `.tvault` backups, atomic restore,
  automatic schedules, and retention controls.
- Global command palette, quick note/task capture, desktop action integration, and configurable
  application preferences.
- Stable **Tenjee Vault 1.0.0** product metadata and portable, per-platform build targets
  (no-install executables; each OS builds only its own target).

## Third-party software

Tenjee Vault is built with Rust, Tauri, React, TipTap, SQLite, and the dependencies recorded in
[Cargo.lock](src-tauri/Cargo.lock) and [package-lock.json](package-lock.json). Their licenses are
provided by their respective authors; the bundled Noto Sans CJK font license is included at
[src-tauri/assets/fonts/NotoSansCJK-LICENSE.txt](src-tauri/assets/fonts/NotoSansCJK-LICENSE.txt).
