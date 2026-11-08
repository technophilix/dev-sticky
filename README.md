# DevSticky

Sticky notes for developers on Linux, built with Rust, GTK4 and libadwaita.

- Markdown notes with syntax-highlighted code blocks and a copy button on each
- Tags and fast full-text search (`docker`, `#bug`, `auth #backend`)
- Lazily loaded list that stays fast with thousands of notes
- To-do lists: tick checkboxes right in the preview, progress shown in the list
- Right-click a note in the list to edit or delete it
- Note colors, pinning, autosave
- Top-bar icon: click to open a quick search panel, click again (or Esc) to hide
- Fully offline. Move notes between machines with Export / Import (main menu)

Notes are stored in `~/.local/share/devsticky/notes.db` (SQLite).

## Export & import

**Export Notes…** writes every note to a JSON file (`devsticky-YYYY-MM-DD.json`).
**Import Notes…** merges such a file into your notes:

- notes you don't have yet are added
- notes edited more recently in the file replace your copy
- your newer edits are kept, and nothing is ever removed
- a note you deleted stays deleted, unless the file has an edit made after the deletion

Importing the same file twice is harmless.

## Install

Download the package for your distro from the Releases page, then:

| Distro | Command |
|---|---|
| Debian 13+, Ubuntu 24.04+, Mint 22+, Pop!_OS | `sudo apt install ./devsticky_0.1.0-1_amd64.deb` |
| Fedora 41+ | `sudo dnf install ./devsticky-0.1.0-1.x86_64.rpm` |

Older releases (e.g. Debian 12, Ubuntu 22.04) ship GTK/libadwaita versions that are too old.

## Build from source

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev build-essential pkg-config
# Fedora: sudo dnf install gcc gtk4-devel libadwaita-devel gtksourceview5-devel

cargo run --release        # just try it
./scripts/install.sh       # install for your user into ~/.local (no sudo)
./scripts/package.sh       # build .deb and .rpm into dist/
```

Pushing a tag like `v0.1.0` to GitHub builds both packages on Ubuntu 24.04 and
Fedora 41 and attaches them to a GitHub Release (`.github/workflows/release.yml`).

The top-bar icon uses the StatusNotifierItem protocol: it works out of the box on
Ubuntu (AppIndicator extension), KDE, XFCE, Cinnamon and waybar. On stock GNOME,
install the "AppIndicator and KStatusNotifierItem Support" extension. Without a
tray, DevSticky runs as a normal window app.

### Command line

| Command | Effect |
|---|---|
| `devsticky` | Show the notes panel (starts the app if needed) |
| `devsticky --new-note` | Create a note (also in the launcher's right-click menu) |
| `devsticky --new-todo` | Create a to-do list |
| `devsticky --background` | Start with only the top-bar icon (used by Start at Login) |

Tip: bind `devsticky` to a keyboard shortcut in GNOME Settings → Keyboard →
Custom Shortcuts (for example Super+Shift+N) to pop up the panel from anywhere.

## Shortcuts

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+Shift+N | New to-do list |
| Ctrl+T | Add a checkbox to the line, or tick / untick it |
| Enter in a list | Continue the list (empty item ends it) |
| Ctrl+F / start typing | Search (list window) |
| Enter in search | Open first result |
| ↓ in search | Move into the results |
| Esc | Clear search, then hide the panel |
| Ctrl+E | Toggle edit / preview |
| Ctrl+Shift+C | Copy whole note |
| Ctrl+L | Show notes list |
| Ctrl+W | Close window |
| Ctrl+Q | Quit |

## License

MIT, see [LICENSE](LICENSE).

## Roadmap

1. ✅ Local notes, Markdown, code blocks, tags, search
2. ✅ Top-bar (tray) icon with a quick search panel
3. ✅ Offline export / import
4. ✅ .deb and .rpm packages
5. Flatpak (Flathub)
