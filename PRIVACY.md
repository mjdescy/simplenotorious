# Privacy Policy

**Applies to:** Simplenotorious for Linux (GTK4/libadwaita) and macOS (SwiftUI), and the Rust crates published from this repository (`simplenotorious-core`, `simplenotorious-ffi`).

**Last updated:** 2026-11-27

## The short version

Simplenotorious does not collect any information about you. There is no telemetry, no analytics, no crash reporting, no advertising, and no tracking of any kind. Nothing you write is ever sent to the developer.

## What the app stores on your device

Everything the app saves stays on your computer:

- Your notes, in a local SQLite database and in the optional folder of Markdown files.
- Your settings, such as theme, sort order and window state.
- If you sign in, an access token for your own SimpleNote account, stored in a file only your user account can read (mode `0600` on Linux).

On Linux these live in `~/.local/share/simplenotorious` and `~/.config/simplenotorious`. On macOS they live in `~/Library/Application Support`. You can delete them at any time; uninstalling the app does not delete them automatically.

## Network access

The app makes network requests only when it has to talk to SimpleNote, and only to the SimpleNote/Simperium service operated by Automattic:

- to open the sign-in page in your browser and redeem the `email:CODE` code you paste in;
- to sync your notes, when sync is enabled or you ask for it.

Those requests go directly from your device to Automattic. The developer never sees them, and no copy is sent anywhere else. Your use of SimpleNote is governed by Automattic's own privacy policy: <https://automattic.com/privacy/>.

The browser step is handled by your operating system's default browser (through the Flatpak portal on Linux). The app does not read your browser history, cookies or any other browser data.

## What the app does not do

- No accounts with the developer, and no sign-up or email address required.
- No analytics, telemetry, usage statistics, device identifiers or fingerprinting.
- No crash or error reporting. Errors are printed to the terminal and stay on your device.
- No advertising, ad SDKs, or third-party SDKs of any kind.
- No cookies or web beacons. The embedded preview renders only your own notes and loads no remote content.
- No selling or sharing of data, because there is no data to sell or share.

## Distribution channels

The app is distributed through package managers and registries, and those services handle their own data:

- **Homebrew** may record download counts if the user has not opted out of Homebrew analytics. That is Homebrew's policy, not the app's.
- **crates.io** and **Flathub** log downloads under their own privacy policies.
- **GitHub** hosts the source code and releases; GitHub logs visits under its privacy policy.

The app itself does not contact any of these services. Your operating system may also check for updates or scan downloaded software (for example macOS Gatekeeper and XProtect); that is done by the OS, not by the app.

## Children

The app is not directed at children and collects no personal information from anyone, including children.

## Changes

If this ever changes — for example, if a future version adds an optional feature that sends data somewhere — this document will be updated before that version is released, and the change will be described in the release notes.

## Contact

Questions about this policy: open an issue at <https://github.com/mjdescy/simplenotorious/issues>.

Simplenotorious is an independent project. It is not affiliated with Simplenote or its owner Automattic in any way.
