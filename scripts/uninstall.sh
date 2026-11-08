#!/usr/bin/env sh
# Uninstalls DevSticky.
#   ./scripts/uninstall.sh           remove the app, keep your notes
#   ./scripts/uninstall.sh --purge   also delete your notes (~/.local/share/devsticky)
#
# Removes the per-user install made by scripts/install.sh, the "Start at Login"
# entry, and (with sudo) the .deb / .rpm package if one is installed.
set -eu
APP_ID=io.github.technophilix.DevSticky
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}"

PURGE=false
for arg in "$@"; do
  case "$arg" in
    --purge) PURGE=true ;;
    -h|--help) sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg (try --help)" >&2; exit 1 ;;
  esac
done

# Stop the running app so it doesn't stay in the top bar.
pkill -x devsticky 2>/dev/null || true

# Per-user install (scripts/install.sh).
rm -f "$BIN_DIR/devsticky" \
      "$DATA_DIR/applications/$APP_ID.desktop"
rm -f "$DATA_DIR"/icons/hicolor/*/apps/"$APP_ID".png
# "Start at Login" entry; package managers don't remove files in your home.
rm -f "$CONFIG_DIR/autostart/$APP_ID.desktop"
gtk4-update-icon-cache -q -t -f "$DATA_DIR/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$DATA_DIR/applications" 2>/dev/null || true

# System package (.deb / .rpm).
if command -v dpkg-query >/dev/null 2>&1 \
   && dpkg-query -W -f='${Status}' devsticky 2>/dev/null | grep -q "install ok installed"; then
  echo "Removing the devsticky .deb package (needs sudo)…"
  sudo apt remove -y devsticky
elif command -v rpm >/dev/null 2>&1 && rpm -q devsticky >/dev/null 2>&1; then
  echo "Removing the devsticky .rpm package (needs sudo)…"
  if command -v dnf >/dev/null 2>&1; then sudo dnf remove -y devsticky; else sudo rpm -e devsticky; fi
fi

NOTES_DIR="$DATA_DIR/devsticky"
if $PURGE; then
  rm -rf "$NOTES_DIR"
  echo "DevSticky and all your notes have been removed."
else
  echo "DevSticky has been uninstalled."
  [ -d "$NOTES_DIR" ] && echo "Your notes are kept in $NOTES_DIR (run with --purge to delete them)."
fi
exit 0
