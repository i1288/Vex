#!/bin/sh
# Install Vex for the current user: binary, icon and app-menu entry.
# GNOME and KDE show the icon from this entry (on Wayland the window's own
# icon is ignored), so run this once after extracting the package.
set -e
cd "$(dirname "$0")"
BIN="${HOME}/.local/bin"
APPS="${HOME}/.local/share/applications"
ICONS="${HOME}/.local/share/icons/hicolor"
mkdir -p "$BIN" "$APPS" "$ICONS/256x256/apps"
install -m 755 vex "$BIN/vex"
install -m 644 vex.png "$ICONS/256x256/apps/vex.png"
# Point the menu entry at the installed binary so it works even when
# ~/.local/bin is not on the desktop session's PATH.
sed "s|^Exec=vex|Exec=$BIN/vex|" vex.desktop > "$APPS/vex.desktop"
chmod 644 "$APPS/vex.desktop"
# Let the desktop pick up the new icon and entry right away.
touch "$ICONS"
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t -f "$ICONS" 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS" 2>/dev/null || true
echo "Installed Vex to $BIN/vex"
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "Note: add $BIN to your PATH to run 'vex' from a terminal." ;;
esac
