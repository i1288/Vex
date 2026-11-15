#!/bin/sh
# Package a universal (Apple Silicon + Intel) Vex.app on macOS:
# dist/vex-<version>-macos-universal.zip
# Usage: scripts/package-macos.sh <version>   (needs both targets built with --release)
set -e
VERSION="${1:?version}"
NAME="vex-${VERSION}-macos-universal"
cd "$(dirname "$0")/.."
APP="dist/Vex.app"
rm -rf "$APP" dist/vex.iconset
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" dist/vex.iconset

lipo -create \
    target/aarch64-apple-darwin/release/vex \
    target/x86_64-apple-darwin/release/vex \
    -output "$APP/Contents/MacOS/vex"
sed "s/@VERSION@/${VERSION#v}/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

for s in 16 32 128 256 512; do
    sips -z $s $s assets/icon.png --out "dist/vex.iconset/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) assets/icon.png --out "dist/vex.iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns dist/vex.iconset -o "$APP/Contents/Resources/vex.icns"

# Ad-hoc signature: required for Apple Silicon (not a Developer ID signature).
codesign --force --deep --sign - "$APP"
ditto -c -k --keepParent "$APP" "dist/$NAME.zip"
echo "dist/$NAME.zip"
