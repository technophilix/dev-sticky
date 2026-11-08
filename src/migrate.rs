//! One-time upgrade from when the app was called "DevNotes".

use gtk::glib;
use std::path::Path;

const OLD_DATA_DIR: &str = "devnotes";
const OLD_AUTOSTART: &str = "io.github.technophilix.DevNotes.desktop";

/// Moves `old` to `new` if only the old one exists. Returns true if it moved.
fn move_dir_if_needed(old: &Path, new: &Path) -> std::io::Result<bool> {
    if new.exists() || !old.join("notes.db").exists() {
        return Ok(false);
    }
    std::fs::rename(old, new)?;
    Ok(true)
}

/// Moves the old notes folder (~/.local/share/devnotes → devsticky) and
/// re-creates the old "Start at Login" entry under the new name.
pub fn run(new_data_dir: &Path) {
    let old = glib::user_data_dir().join(OLD_DATA_DIR);
    match move_dir_if_needed(&old, new_data_dir) {
        Ok(true) => eprintln!(
            "devsticky: moved notes from {} to {}",
            old.display(),
            new_data_dir.display()
        ),
        Ok(false) => {}
        Err(e) => eprintln!("devsticky: could not move old notes from {}: {e}", old.display()),
    }

    let old_autostart = glib::user_config_dir().join("autostart").join(OLD_AUTOSTART);
    if old_autostart.exists() {
        let _ = std::fs::remove_file(&old_autostart);
        if let Err(e) = crate::autostart::set_enabled(true) {
            eprintln!("devsticky: could not update Start at Login: {e:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_only_when_new_is_missing() {
        let tmp = std::env::temp_dir().join(format!("devsticky-migrate-{}", std::process::id()));
        let (old, new) = (tmp.join("devnotes"), tmp.join("devsticky"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("notes.db"), b"db").unwrap();

        assert!(move_dir_if_needed(&old, &new).unwrap());
        assert_eq!(std::fs::read(new.join("notes.db")).unwrap(), b"db");
        assert!(!old.exists());

        // Second run, or both folders present: never touch anything.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("notes.db"), b"other").unwrap();
        assert!(!move_dir_if_needed(&old, &new).unwrap());
        assert_eq!(std::fs::read(new.join("notes.db")).unwrap(), b"db");

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
