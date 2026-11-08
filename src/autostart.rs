//! "Start at login" via an XDG autostart entry in ~/.config/autostart.

use anyhow::{Context, Result};
use gtk::glib;
use std::path::PathBuf;

use crate::APP_ID;

fn entry_path() -> PathBuf {
    glib::user_config_dir()
        .join("autostart")
        .join(format!("{APP_ID}.desktop"))
}

pub fn is_enabled() -> bool {
    entry_path().exists()
}

/// Quotes a path for a desktop entry `Exec=` line (per the Desktop Entry spec).
fn quote_exec_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn entry_contents(exe: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=DevSticky\n\
         Comment=Sticky notes for developers\n\
         Exec={} --background\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        quote_exec_arg(exe)
    )
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = entry_path();
    if !enabled {
        if path.exists() {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
        return Ok(());
    }
    let exe = std::env::current_exe().context("finding the DevSticky executable")?;
    let exe = exe.to_str().context("executable path is not valid UTF-8")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, entry_contents(exe))
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_line_is_quoted() {
        assert_eq!(quote_exec_arg("/home/a b/devsticky"), "\"/home/a b/devsticky\"");
        assert_eq!(quote_exec_arg("/x/$y\"z"), "\"/x/\\$y\\\"z\"");
        assert!(entry_contents("/usr/bin/devsticky").contains("Exec=\"/usr/bin/devsticky\" --background\n"));
    }
}
