#!/usr/bin/env bash
# Installs the app icon and launcher entry for the current Linux user.
# Build first: cargo build --release -p cominorsa-desktop
set -euo pipefail
cd "$(dirname "$0")/.."

binary="$PWD/target/release/cominorsa-desktop"
[[ -x "$binary" ]] || { echo "Missing $binary: build the release first" >&2; exit 1; }

data="${XDG_DATA_HOME:-$HOME/.local/share}"
install -Dm644 crates/desktop/assets/icon.png "$data/icons/hicolor/512x512/apps/cominorsa-desktop.png"
install -d "$data/applications"
cat > "$data/applications/cominorsa-desktop.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=COMINORSA CRM
Comment=CRM local de COMINORSA
Exec=$binary
Icon=cominorsa-desktop
Terminal=false
Categories=Office;
StartupWMClass=cominorsa-desktop
EOF

command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" || true
echo "Installed launcher in $data/applications"
