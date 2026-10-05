# Android development

Status: development target, not a released mobile version. The shared React/Rust app now has an Android Studio project in `src-tauri/gen/android`. Android native build artifacts do not constitute acceptance of document sharing, suspended reminders, or LAN exchange.

## Toolchain

This workspace uses Java 17, Android SDK platform/build tools 36, NDK 28.2.13676358, and Rust targets `aarch64-linux-android` and `x86_64-linux-android`. The generated project pins Gradle 8.14.3 with its distribution checksum and Android Gradle plugin 8.11.0. Its minimum Android API is 24; older-device/WebView coverage remains pending.

The npm Android commands use `$HOME/Android/Sdk` on Linux by default. Set `ANDROID_HOME` or `ANDROID_SDK_ROOT` for another SDK location and `NDK_HOME` for another installation of the pinned NDK. Gradle also pins that NDK version for symbol stripping. These commands change only their child process environment, not shell settings or global Gradle configuration. See [Tauri's Android prerequisites](https://v2.tauri.app/start/prerequisites/#android).

```bash
# Install missing Rust targets. Use the official server if a configured mirror
# does not carry your Rust toolchain's Android components.
RUSTUP_DIST_SERVER=https://static.rust-lang.org rustup target add aarch64-linux-android x86_64-linux-android

# Install the NDK if it is missing (current SDK CLI uses slash package paths).
"$HOME/Android/Sdk/cmdline-tools/latest/bin/sdkmanager" "ndk/28.2.13676358"
```

An existing unauthenticated HTTP proxy in `HTTPS_PROXY`/`https_proxy` is forwarded to Java/Gradle, including its daemon. Localhost connections bypass it. Explicit Java/Gradle proxy settings take precedence. Proxy URLs with credentials are not forwarded into JVM options; configure an authenticated proxy using your own secure Gradle setup. Build downloads through that proxy use TLS 1.2 to accommodate this workspace’s Maven connection; TLS certificate verification remains enabled. LAN pairing requires TLS 1.3.

## Build and run

```bash
# ARM64 phone APK, with bundled frontend assets.
npm run android:apk -- --ci

# x86_64 emulator APK.
npm run android:emulator -- --ci

# Live development on a connected device/emulator.
npm run android:dev

# Production build commands; release signing still needs your own configuration.
npm run android:build -- --target aarch64 --apk --aab --ci
```

Debug APKs use application ID `com.sam.tenjee_vault.debug`, separate from the release application ID `com.sam.tenjee_vault`. Successful debug builds copy APKs to `artifacts/android/tenjee-vault-debug-aarch64.apk` and `artifacts/android/tenjee-vault-debug-x86_64.apk`. Native symbols are stripped from installable debug APKs; unstripped libraries remain in Cargo’s target directory. Set `ORG_GRADLE_PROJECT_keepRustDebugSymbols=true` when building to retain symbols inside an APK for native debugging. Build output, machine paths, generated Kotlin bindings, and signing credentials are excluded from Git; the Android project sources and wrapper are tracked.

```bash
# Connect a phone with USB debugging enabled, then select its serial if needed.
"$HOME/Android/Sdk/platform-tools/adb" devices -l
"$HOME/Android/Sdk/platform-tools/adb" -s DEVICE_SERIAL install -r artifacts/android/tenjee-vault-debug-aarch64.apk

# The installed Pixel emulator in this workspace:
"$HOME/Android/Sdk/emulator/emulator" -avd pixel36
"$HOME/Android/Sdk/platform-tools/adb" -s emulator-5554 install -r artifacts/android/tenjee-vault-debug-x86_64.apk
"$HOME/Android/Sdk/platform-tools/adb" -s emulator-5554 shell am start -n com.sam.tenjee_vault.debug/com.sam.tenjee_vault.MainActivity
```

`android:init` is for generating a new target project; it is not needed for routine builds of the checked-in project. Regeneration can overwrite the customized manifest and MainActivity, so preserve/reapply those changes if regenerating.

## Implemented Android integrations

- Private app storage holds databases and attachment blobs. Android cloud backup and automatic device transfer are disabled. Explicit `.tvault` backup/restore remains available.
- Native system-bar, cutout, and keyboard insets keep the WebView usable. Bottom tabs hide while typing. Android Back dismisses sheets, waits for note saves, then navigates or exits; save failures keep the screen open.
- Imports use Android's system document picker and copy selected documents into a private temporary workspace. Exports use the system save dialog. Attachment opening/sharing uses a FileProvider and temporary URI grants. Protected attachment sharing requires explicit plaintext consent. Temporary sharing files expire after ten minutes; a receiver can retain its own copy.
- Automatic backups use a persistently authorized system folder. Verified local backups are copied to that provider with retention limited to files created by this app. Incomplete provider copies are cleaned on retry; losing folder access is reported rather than silently selecting another folder.
- Task/calendar reminders use AlarmManager and durable native scheduling state. Notification permission is requested after application setup. Exact-alarm access is optional; when unavailable the app uses Android's inexact delivery. Boot, clock/timezone, package, and exact-permission changes reinstall the schedule. Device-local fired history prevents duplicate notifications and does not synchronize.
- Scheduling covers up to 256 upcoming/catch-up alarms through a maximum 30-day window. Settings displays the effective window and permission status; reopen the app to extend the window. Android force-stop, battery controls, and inexact scheduling can delay or prevent delivery and require physical validation.
- Debug and release builds expose foreground LAN exchange following user-reported successful testing on 2026-10-05. Native activity suspension cancels it and releases the multicast lock independently of JavaScript. The app does not open a sync listener at ordinary startup.
- Protected page titles are encrypted. Existing titles migrate when their tree is locally unlocked; locked titles display neutral labels. Unmigrated protected groups remain pending during exchange.

Current target SDK is 36. Android 16 normally permits local networking through INTERNET; targeting Android 17/API 37 will require the new local-network permission flow. See [Android local-network permission guidance](https://developer.android.com/privacy-and-security/local-network-permission).

## Validation status

Frontend, Linux Rust, and ARM64/x86_64 Android development builds are recorded in [implementation notes](../openspec/changes/mobile-and-lan-sync/implementation-notes.md). Earlier emulator observations covered startup, default-space creation, gesture insets, and keyboard-aware quick capture; they preceded these native integrations and do not establish their acceptance.

The user reported successful Device Exchange testing on 2026-10-05 and requested release-build availability. The broader physical phone and protocol acceptance checklist remains open. iOS still requires implementation and a Mac/Xcode runner. See [the phone validation checklist](android-validation.md) and [LAN implementation notes](lan-pairing-implementation.md). OpenSpec acceptance tasks remain open.
