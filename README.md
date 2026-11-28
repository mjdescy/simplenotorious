# Simplenotorious

A native, non-Electron Markdown notes app for [SimpleNote](https://simplenote.com/).
Linux: GTK4 + libadwaita (GNOME/Adwaita look) with WebKitGTK preview. macOS: SwiftUI with WKWebView preview.
Both UIs share one Rust core (`crates/core`) that uses the [`simplenote-cli`](https://crates.io/crates/simplenote-cli) library for the SimpleNote API.

## Features
- Log in / log out, first-run login screen (SimpleNote's browser flow: open the login page, paste the `email:CODE` shown).
- Collapsible sidebar (pinned notes, title + snippet, search, tag/trash filter), Markdown editor, read-only HTML preview toggle (Ctrl/⌘+P).
- Light / Dark / Follow-system theme. Preferences: **Ctrl+,** (Linux) / **⌘,** (macOS).
- **Storage**: SQLite database (fast) is the source of truth; every edit is committed immediately.
  - Mirrored two-way to a folder of plain `.md` files (`<title>.md`; metadata in hidden `.simplenotorious.json`) so other tools can sync them. External edits, new files and deleted files (→ trash) are picked up.
  - Optional "keep `notes.json` in sync": writes a SimpleNote-format export (`activeNotes` / `trashedNotes`) after every change. Import/Export from the menu.
- **Sync** with SimpleNote is offline-tolerant: edits are saved locally and marked dirty; failures back off exponentially and retry; nothing is dropped. Conflicts keep both versions (your local edit becomes a new note tagged `conflict`).

## Layout
```
crates/core    shared logic (store, folder mirror, JSON export, sync, markdown, settings) + tests
crates/ffi     C ABI (JSON in/out) used by the SwiftUI app
crates/linux   GTK4/libadwaita app   (not part of the default workspace build)
macos/         SwiftUI app + build.sh
```

## Build & run — Linux
Needs libadwaita ≥ 1.5, GTK 4.10+, WebKitGTK 6 and GtkSourceView 5 development files:
```
# Debian/Ubuntu
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libgtksourceview-5-dev
# Fedora
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libadwaita-devel webkitgtk6.0-devel gtksourceview5-devel
cd crates/linux && cargo run --release
```
### Linux on an immutable distro (Bluefin, Silverblue, …): devcontainer
`.devcontainer/` provides a Fedora image with all GTK/WebKit dev packages and Rust.
Open the folder in VS Code (Dev Containers extension, with `"dev.containers.dockerPath": "podman"`),
or from a terminal: `devcontainer up --workspace-folder . --docker-path podman`. Then, inside the container:
```
cd crates/linux && cargo run --release   # window appears on your host Wayland session
cargo test --workspace                   # core tests
```
Notes/login persist in named volumes across rebuilds. The container disables WebKit's nested sandbox (it can't run inside a container).

## Flatpak releases
Pushing a tag that starts with `v` builds a Flatpak bundle in GitHub Actions and attaches it to a GitHub release:
```
git tag v0.2.0 && git push origin v0.2.0
```
Users install it with `flatpak install --user simplenotorious-v0.2.0.flatpak` (the GNOME 49 runtime comes from Flathub).
The recipe is in `flatpak/`; `flatpak/build-local.sh` builds and installs it locally. The workflow is `.github/workflows/flatpak.yml`.

## Build & run — macOS (Xcode 15+, rustup)
```
cd macos && ./build.sh && open Simplenotorious.app
```
## Tests
```
cargo test --workspace      # core + ffi (no GUI libraries needed)
```
Data locations — Linux: `~/.local/share/simplenotorious` (database, default notes folder) and `~/.config/simplenotorious` (settings, `auth.json` token, mode 0600). macOS: the same two folders under `~/Library/Application Support`.

## Running outside the container (to test login, browser links, etc.)
Build in the container, run on the host (needs GTK4, libadwaita ≥1.5, WebKitGTK 6, GtkSourceView 5 on the host — present on Bluefin):
```
mkdir -p dist
podman run --rm --userns=keep-id --security-opt label=disable -v "$PWD":/w:Z -v sn-target:/tmp/target \
  -e CARGO_TARGET_DIR=/tmp/target -w /w/crates/linux simplenotorious-dev \
  bash -c 'cargo build --release && cp /tmp/target/release/simplenotorious /w/dist/'
./dist/simplenotorious
```
(Or inside the devcontainer: `cargo build --release`, then copy the binary from `$CARGO_TARGET_DIR/release/` to the repo.)

## Privacy
The app collects nothing: no telemetry, no analytics, no accounts with the developer. See [PRIVACY.md](PRIVACY.md).

## License
MIT — see [LICENSE.md](LICENSE.md).
