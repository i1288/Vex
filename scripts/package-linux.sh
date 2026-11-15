#!/bin/sh
# Package a release build for Linux: dist/vex-<version>-linux-x86_64.tar.gz
# Usage: scripts/package-linux.sh <version>   (after `cargo build --release`)
set -e
VERSION="${1:?version}"
NAME="vex-${VERSION}-linux-x86_64"
cd "$(dirname "$0")/.."
rm -rf "dist/$NAME"
mkdir -p "dist/$NAME"
cp target/release/vex "dist/$NAME/"
strip "dist/$NAME/vex" 2>/dev/null || true
cp assets/icon-256.png "dist/$NAME/vex.png"
cp packaging/linux/vex.desktop packaging/linux/install.sh README.md "dist/$NAME/"
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
echo "dist/$NAME.tar.gz"
