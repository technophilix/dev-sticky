# DevNotes

Sticky notes for developers on Linux, built with Rust, GTK4 and libadwaita.

- Markdown notes with syntax-highlighted code blocks and a copy button on each
- Tags and fast full-text search (`docker`, `#bug`, `auth #backend`)
- Lazily loaded list that stays fast with thousands of notes
- Note colors, pinning, autosave
- Top-bar icon: click to open a quick search panel, click again (or Esc) to hide
- Fully offline. Move notes between machines with Export / Import (main menu)

Notes are stored in `~/.local/share/devnotes/notes.db` (SQLite).

## Export & import

**Export Notes…** writes every note to a JSON file (`devnotes-YYYY-MM-DD.json`).
**Import Notes…** merges such a file into your notes:

- notes you don't have yet are added
- notes edited more recently in the file replace your copy
- your newer edits are kept, and nothing is ever removed
- a note you deleted stays deleted, unless the file has an edit made after the deletion

Importing the same file twice is harmless.

## Build & install

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev build-essential pkg-config
./scripts/install.sh      # builds and installs to ~/.local (no sudo)
```

Or just `cargo run --release` to try it.

The top-bar icon uses the StatusNotifierItem protocol: it works out of the box on
Ubuntu (AppIndicator extension), KDE, XFCE, Cinnamon and waybar. On stock GNOME,
install the "AppIndicator and KStatusNotifierItem Support" extension. Without a
tray, DevNotes runs as a normal window app.

### Command line

| Command | Effect |
|---|---|
| `devnotes` | Show the notes panel (starts the app if needed) |
| `devnotes --new-note` | Create a note (also in the launcher's right-click menu) |
| `devnotes --background` | Start with only the top-bar icon (used by Start at Login) |

Tip: bind `devnotes` to a keyboard shortcut in GNOME Settings → Keyboard →
Custom Shortcuts (for example Super+Shift+N) to pop up the panel from anywhere.

## Shortcuts

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+F / start typing | Search (list window) |
| Enter in search | Open first result |
| ↓ in search | Move into the results |
| Esc | Clear search, then hide the panel |
| Ctrl+E | Toggle edit / preview |
| Ctrl+Shift+C | Copy whole note |
| Ctrl+L | Show notes list |
| Ctrl+W | Close window |
| Ctrl+Q | Quit |

## Roadmap

1. ✅ Local notes, Markdown, code blocks, tags, search
2. ✅ Top-bar (tray) icon with a quick search panel
3. ✅ Offline export / import
