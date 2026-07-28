#!/usr/bin/env bash
# OpenShark Desktop — install app launcher + icons (idempotent).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="$REPO_DIR/src-tauri/target/release/openshark-desktop"
ICON_SRC="$REPO_DIR/src-tauri/icons/icon.png"
APPS_DIR="$HOME/.local/share/applications"
ICON_BASE="$HOME/.local/share/icons/hicolor"

if [[ ! -x "$BINARY" ]]; then
  echo "Binary not found at $BINARY — run: npm run tauri build" >&2
  exit 1
fi

# Icons at standard sizes (from the master icon)
for size in 32 64 128 256 512; do
  dir="$ICON_BASE/${size}x${size}/apps"
  mkdir -p "$dir"
  magick "$ICON_SRC" -resize "${size}x${size}" "$dir/openshark-desktop.png"
done

# Launcher
mkdir -p "$APPS_DIR"
cat > "$APPS_DIR/openshark-desktop.desktop" <<EOF
[Desktop Entry]
Name=OpenShark Desktop
Comment=GUI for OpenShark — the harness that learns
Exec=$BINARY
Icon=openshark-desktop
Type=Application
Categories=Development;Utility;
StartupWMClass=openshark-desktop
Terminal=false
EOF

# Refresh caches (KDE/GTK)
gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true
kbuildsycoca6 --noincremental 2>/dev/null || true
update-desktop-database "$APPS_DIR" 2>/dev/null || true

echo "🦈 Installed launcher + icons. Binary: $BINARY"
