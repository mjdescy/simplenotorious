app_id := "me.mjdescy.Simplenotorious"

default:
    @just --list

# Build the Flatpak and install it for the current user
build:
    bash flatpak/build-local.sh

# Run the installed Flatpak
run:
    flatpak run {{app_id}}

# Build, then run
br: build run

# Regenerate the third-party licenses shown in the About dialog (needs cargo-about)
licenses:
    nu scripts/gen-licenses.nu

# Run the core tests (no GUI libraries needed)
test:
    cargo test --workspace

# Uninstall the Flatpak
uninstall:
    flatpak uninstall --user -y {{app_id}}
