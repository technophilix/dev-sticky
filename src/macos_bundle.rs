//! Running from DevSticky.app: point GTK at the data files bundled in
//! `Contents/Resources` (icons, GSettings schemas, GtkSourceView languages and
//! the SVG image loader), so the app works without Homebrew installed.
//!
//! Does nothing when not running from a bundle (e.g. `cargo run`).

use std::path::{Path, PathBuf};

/// Placeholder in the bundled `loaders.cache.in` for the Resources path.
const RESOURCES_PLACEHOLDER: &str = "@RESOURCES@";

/// Must run first thing in `main`, before any other thread exists.
pub fn setup_env() {
    let Some(resources) = bundle_resources() else {
        return;
    };
    let share = resources.join("share");
    // SAFETY: called at the very start of `main`, while the process is single-threaded.
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", &share);
        std::env::set_var("GSETTINGS_SCHEMA_DIR", share.join("glib-2.0/schemas"));
    }

    // gdk-pixbuf's loaders.cache must list absolute paths, and the app can live
    // anywhere, so the cache is written from a template on each start.
    let template = resources.join("lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in");
    let Ok(text) = std::fs::read_to_string(&template) else {
        return;
    };
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let cache_dir = Path::new(&home).join("Library/Caches").join(crate::APP_ID);
    let cache = cache_dir.join("loaders.cache");
    let contents = fill_template(&text, &resources);
    let written = std::fs::create_dir_all(&cache_dir).and_then(|()| std::fs::write(&cache, contents));
    match written {
        // SAFETY: as above.
        Ok(()) => unsafe { std::env::set_var("GDK_PIXBUF_MODULE_FILE", &cache) },
        Err(e) => eprintln!("devsticky: writing {} failed: {e}", cache.display()),
    }
}

/// `…/DevSticky.app/Contents/Resources` if the executable is inside an app bundle.
fn bundle_resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let resources = macos_dir.parent()?.join("Resources");
    resources.join("share").is_dir().then_some(resources)
}

fn fill_template(text: &str, resources: &Path) -> String {
    text.replace(RESOURCES_PLACEHOLDER, &resources.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_gets_resources_path() {
        let t = "\"@RESOURCES@/lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader_svg.so\"\n";
        let out = fill_template(t, Path::new("/Applications/DevSticky.app/Contents/Resources"));
        assert_eq!(
            out,
            "\"/Applications/DevSticky.app/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader_svg.so\"\n"
        );
    }
}
