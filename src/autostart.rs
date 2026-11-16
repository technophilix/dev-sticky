//! "Start at login".
//!
//! Linux: an XDG autostart entry in ~/.config/autostart.
//! macOS: a LaunchAgent in ~/Library/LaunchAgents.

use anyhow::{Context, Result};
#[cfg(not(target_os = "macos"))]
use gtk::glib;
use std::path::PathBuf;

use crate::APP_ID;

#[cfg(not(target_os = "macos"))]
fn entry_path() -> PathBuf {
    glib::user_config_dir()
        .join("autostart")
        .join(format!("{APP_ID}.desktop"))
}

#[cfg(target_os = "macos")]
fn entry_path() -> PathBuf {
    gtk::glib::home_dir()
        .join("Library/LaunchAgents")
        .join(format!("{APP_ID}.plist"))
}

pub fn is_enabled() -> bool {
    entry_path().exists()
}

/// Quotes a path for a desktop entry `Exec=` line (per the Desktop Entry spec).
#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
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

/// Escapes text for an XML property list.
#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(target_os = "macos")]
fn entry_contents(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{APP_ID}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>--background</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        xml_escape(exe)
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

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn exec_line_is_quoted() {
        assert_eq!(quote_exec_arg("/home/a b/devsticky"), "\"/home/a b/devsticky\"");
        assert_eq!(quote_exec_arg("/x/$y\"z"), "\"/x/\\$y\\\"z\"");
        assert!(entry_contents("/usr/bin/devsticky").contains("Exec=\"/usr/bin/devsticky\" --background\n"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_escapes_path() {
        let plist = entry_contents("/Users/a&b/devsticky");
        assert!(plist.contains("<string>/Users/a&amp;b/devsticky</string>"));
        assert!(plist.contains("<string>--background</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
    }
}
