#!/bin/bash
# Build and sign a staged bundle before replacing the last working app.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
PROFILE="release"
BUILD_FLAGS=(--release)
case "${1:-}" in
  "") ;;
  --debug) PROFILE="debug"; BUILD_FLAGS=() ;;
  *) echo "用法: $0 [--debug]" >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then
  echo "用法: $0 [--debug]" >&2
  exit 2
fi
if [ "$(uname -s)" != "Darwin" ]; then
  echo "此打包脚本仅支持 macOS" >&2
  exit 1
fi
cargo build --manifest-path "$ROOT/src-tauri/Cargo.toml" --locked ${BUILD_FLAGS[@]+"${BUILD_FLAGS[@]}"}
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/src-tauri/target}"
if [[ "$TARGET_DIR" != /* ]]; then TARGET_DIR="$PWD/$TARGET_DIR"; fi
BIN="$TARGET_DIR/$PROFILE/desktop-pet-clipboard"
APP="$ROOT/Desktop Pet Clipboard.app"
STAGE="$(mktemp -d "$ROOT/.desktop-pet-build.XXXXXX")"
STAGED_APP="$STAGE/Desktop Pet Clipboard.app"
CONTENTS="$STAGED_APP/Contents"
cleanup() {
  if [ -e "$STAGE/previous.app" ] && [ ! -e "$APP" ]; then
    if ! mv "$STAGE/previous.app" "$APP"; then
      printf '旧应用保留在: %s\n' "$STAGE/previous.app" >&2
      return
    fi
  fi
  rm -rf "$STAGE"
}
trap cleanup EXIT
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
cp "$BIN" "$CONTENTS/MacOS/desktop-pet-clipboard"

cat > "$CONTENTS/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>desktop-pet-clipboard</string>
  <key>CFBundleIdentifier</key><string>com.yuping.desktop-pet-clipboard</string>
  <key>CFBundleName</key><string>Desktop Pet Clipboard</string>
  <key>CFBundleDisplayName</key><string>宠物剪贴板</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

plutil -lint "$CONTENTS/Info.plist"
codesign --force --deep --sign - "$STAGED_APP"
codesign --verify --deep --strict "$STAGED_APP"
if [ -e "$APP" ]; then mv "$APP" "$STAGE/previous.app"; fi
mv "$STAGED_APP" "$APP"
printf '打包完成 (%s): %s\n' "$PROFILE" "$APP"
printf '重新构建后，macOS 可能要求重新授予辅助功能权限。\n'
