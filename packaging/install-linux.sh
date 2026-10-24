#!/bin/sh
set -e
cd "$(dirname "$0")"

BIN=~/.local/bin
APPS=~/.local/share/applications
ICONS=~/.local/share/icons/hicolor

mkdir -p "$BIN" "$APPS"
for s in 16 32 48 64 128 256; do
    d="$ICONS/${s}x${s}/apps"
    mkdir -p "$d"
    cp io.alpaca.rs-$s.png "$d/io.alpaca.rs.png"
done
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$ICONS" 2>/dev/null || true

cp io.alpaca.rs.desktop "$APPS/"
cp alpaca-code-rs "$BIN/"
echo "installed: $BIN/alpaca-code-rs (+ app icon, launcher entry)"