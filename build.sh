#!/usr/bin/env bash
set -euo pipefail

PROJECT="cosmic-chat"
BINARY="cosmic-chat"
WORKSPACE="$(cd "$(dirname "$0")" && pwd)"
TARGET_DIR="$WORKSPACE/target"

VERSION="0.1.0"
MODE="${1:-debug}"

echo "=== $PROJECT v$VERSION ==="
echo ""

if [ -f "$WORKSPACE/CHANGELOG.md" ]; then
    echo "--- Recent changes ---"
    sed -n '/^## /{p;n;p;n;p;n;p;n;p;n;p;n;p;n;p;q}' "$WORKSPACE/CHANGELOG.md"
    echo ""
fi

echo "=== Building ($MODE) ==="
cd "$WORKSPACE"

if [ "$MODE" = "release" ]; then
    cargo build --release
    SRC="$TARGET_DIR/release/$BINARY"
else
    cargo build
    SRC="$TARGET_DIR/debug/$BINARY"
fi

echo ""
echo "=== Build complete ==="
ls -lh "$SRC"

ICON="$WORKSPACE/resources/icons/hicolor/scalable/apps/icon.svg"
DESKTOP="$WORKSPACE/resources/cosmic-chat.desktop"

echo ""
echo "=== Installing ==="
mkdir -p ~/.local/bin ~/.local/share/icons/hicolor/scalable/apps ~/.local/share/applications
cp "$SRC"     ~/.local/bin/$BINARY
cp "$ICON"    ~/.local/share/icons/hicolor/scalable/apps/$BINARY.svg
cp "$DESKTOP" ~/.local/share/applications/$BINARY.desktop
update-desktop-database ~/.local/share/applications 2>/dev/null || true

echo "  binary     -> ~/.local/bin/$BINARY"
echo "  icon       -> ~/.local/share/icons/hicolor/scalable/apps/$BINARY.svg"
echo "  desktop    -> ~/.local/share/applications/$BINARY.desktop"
echo ""
echo "=== Done: $BINARY v$VERSION installed ==="
