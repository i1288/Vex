#!/bin/sh
# Install Vex for the current user: binary, icon and app-menu entry.
set -e
cd "$(dirname "$0")"
BIN="${HOME}/.local/bin"
mkdir -p "$BIN" "${HOME}/.local/share/applications" "${HOME}/.local/share/icons/hicolor/256x256/apps"
install -m 755 vex "$BIN/vex"
install -m 644 vex.png "${HOME}/.local/share/icons/hicolor/256x256/apps/vex.png"
install -m 644 vex.desktop "${HOME}/.local/share/applications/vex.desktop"
echo "Installed Vex to $BIN/vex"
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "Note: add $BIN to your PATH to run 'vex' from a terminal." ;;
esac
