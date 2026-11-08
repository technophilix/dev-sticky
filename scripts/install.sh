#!/usr/bin/env sh
# Builds DevSticky and installs it for the current user (no sudo needed):
#   ~/.local/bin/devsticky
#   ~/.local/share/applications/io.github.technophilix.DevSticky.desktop
#   ~/.local/share/icons/hicolor/<size>/apps/io.github.technophilix.DevSticky.png
set -eu
cd "$(dirname "$0")/.."
APP_ID=io.github.technophilix.DevSticky
BIN_DIR="${HOME}/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"

# Terminals opened before installing Rust don't have ~/.cargo/bin on PATH yet.
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
  . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust from https://rustup.rs and try again." >&2
  exit 1
fi

cargo build --release
install -Dm755 target/release/devsticky "$BIN_DIR/devsticky"
# Use the full path: GNOME hides launchers whose program isn't on its PATH, and
# ~/.local/bin is only added to PATH at login if it already existed.
mkdir -p "$DATA_DIR/applications"
sed "s|^Exec=devsticky|Exec=\"$BIN_DIR/devsticky\"|" "data/$APP_ID.desktop" \
  > "$DATA_DIR/applications/$APP_ID.desktop"
for png in data/icons/hicolor/*/apps/"$APP_ID".png; do
  size_dir=$(basename "$(dirname "$(dirname "$png")")")
  install -Dm644 "$png" "$DATA_DIR/icons/hicolor/$size_dir/apps/$APP_ID.png"
done
# Remove files from earlier installs (placeholder icon, and the old "DevNotes" name).
rm -f "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg" \
      "$BIN_DIR/devnotes" \
      "$DATA_DIR/applications/io.github.technophilix.DevNotes.desktop" \
      "$DATA_DIR/icons/hicolor/scalable/apps/io.github.technophilix.DevNotes.svg"
rm -f "$DATA_DIR"/icons/hicolor/*/apps/io.github.technophilix.DevNotes.png
gtk4-update-icon-cache -q -t -f "$DATA_DIR/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$DATA_DIR/applications" 2>/dev/null || true

echo "Installed DevSticky to $BIN_DIR/devsticky"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Note: $BIN_DIR is not on your PATH; add it to launch 'devsticky' from a terminal." ;;
esac
echo "Open it from your app launcher, and enable 'Start at Login' in its menu."
