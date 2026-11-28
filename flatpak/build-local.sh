#!/usr/bin/env bash
# Build and install the Flatpak locally (user installation) to test it.
#   needs: flatpak, flatpak-builder, python3 (with the venv module)
set -euo pipefail
cd "$(dirname "$0")"
GEN=flatpak-cargo-generator.py
[ -f "$GEN" ] || curl -sSfLo "$GEN" \
  https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
# The generator's Python deps live in a private venv so the host Python (Homebrew, etc.) needs nothing.
VENV=.venv
if [ ! -x "$VENV/bin/python" ] || ! "$VENV/bin/python" -c 'import aiohttp, tomlkit' 2>/dev/null; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install --quiet aiohttp tomlkit
fi
"$VENV/bin/python" "$GEN" ../crates/linux/Cargo.lock -o cargo-sources.json
# The user installation needs its own Flathub remote (often only system-wide on atomic distros).
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
# Host flatpak-builder crashes without `appstreamcli` (Bluefin, Silverblue, ...), and wrapping the
# SDK's copy fails because its icon loader cannot sandbox itself. Use the Flatpak build of
# flatpak-builder instead, which bundles a working appstreamcli.
if command -v appstreamcli >/dev/null && command -v flatpak-builder >/dev/null; then
  BUILDER=(flatpak-builder)
else
  flatpak info --user org.flatpak.Builder >/dev/null 2>&1 || flatpak install --user -y flathub org.flatpak.Builder
  BUILDER=(flatpak run org.flatpak.Builder)
fi
"${BUILDER[@]}" --user --force-clean --install --install-deps-from=flathub \
  build-dir me.mjdescy.Simplenotorious.yml
echo "Run with: flatpak run me.mjdescy.Simplenotorious"
