//! Top-bar (system tray) icon using the StatusNotifierItem D-Bus protocol.
//!
//! Works with GNOME (AppIndicator extension), KDE, XFCE, Cinnamon and waybar.
//! The tray runs on its own thread; clicks are sent to the GTK main loop as
//! [`TrayEvent`]s together with the Wayland activation token, which lets the
//! window we show take keyboard focus.

use gtk::prelude::*;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;

#[derive(Debug)]
pub enum TrayEvent {
    /// Left click: show the quick panel, or hide it if it is focused.
    Toggle(Option<String>),
    Show(Option<String>),
    NewNote(Option<String>),
    NewTodo(Option<String>),
    Quit,
    /// The desktop no longer shows tray icons.
    Gone,
    /// Tray icons are available (again).
    Back,
}

struct DevStickyTray {
    tx: async_channel::Sender<TrayEvent>,
    icon: Vec<ksni::Icon>,
}

impl DevStickyTray {
    fn send(&self, make: impl FnOnce(Option<String>) -> TrayEvent) {
        let _ = self.tx.send_blocking(make(activation_token()));
    }
}

/// The token can arrive slightly after the click event, so retry briefly.
fn activation_token() -> Option<String> {
    for _ in 0..20 {
        if let Some(t) = ksni::current_xdg_activation_token() {
            return Some(t);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    None
}

impl ksni::Tray for DevStickyTray {
    fn id(&self) -> String {
        "devsticky".into()
    }

    fn title(&self) -> String {
        "DevSticky".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "DevSticky".into(),
            description: "Click to search your notes".into(),
            ..Default::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Toggle);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Search Notes".into(),
                icon_name: "edit-find-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayEvent::Show)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "New Note".into(),
                icon_name: "list-add-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayEvent::NewNote)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "New To-do List".into(),
                icon_name: "object-select-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayEvent::NewTodo)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send_blocking(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        let _ = self.tx.send_blocking(TrayEvent::Gone);
        // Keep the service alive so the icon comes back if the watcher returns.
        true
    }

    fn watcher_online(&self) {
        let _ = self.tx.send_blocking(TrayEvent::Back);
    }
}

pub struct TrayHandle {
    _handle: ksni::blocking::Handle<DevStickyTray>,
}

/// Starts the tray icon. Fails if the desktop has no tray (StatusNotifierHost).
pub fn spawn(tx: async_channel::Sender<TrayEvent>) -> Result<TrayHandle, ksni::Error> {
    let tray = DevStickyTray {
        tx,
        icon: logo_icons(),
    };
    Ok(TrayHandle {
        _handle: tray.spawn()?,
    })
}

/// The app logo at the sizes tray hosts typically ask for, embedded in the binary.
const ICON_PNGS: &[&[u8]] = &[
    include_bytes!("../data/icons/hicolor/22x22/apps/io.github.technophilix.DevSticky.png"),
    include_bytes!("../data/icons/hicolor/32x32/apps/io.github.technophilix.DevSticky.png"),
    include_bytes!("../data/icons/hicolor/64x64/apps/io.github.technophilix.DevSticky.png"),
];

/// Decodes the embedded logo PNGs into ARGB32 (network byte order), which is
/// what StatusNotifierItem expects. Must run on the GTK thread.
pub fn logo_icons() -> Vec<ksni::Icon> {
    ICON_PNGS
        .iter()
        .filter_map(|png| {
            let texture = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from_static(png)).ok()?;
            let (w, h) = (texture.width(), texture.height());
            let mut dl = gtk::gdk::TextureDownloader::new(&texture);
            dl.set_format(gtk::gdk::MemoryFormat::A8r8g8b8);
            let (bytes, stride) = dl.download_bytes();
            let row = w as usize * 4;
            let data = bytes
                .chunks(stride)
                .take(h as usize)
                .flat_map(|r| r[..row].iter().copied())
                .collect();
            Some(ksni::Icon {
                width: w,
                height: h,
                data,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn logo_pngs_decode_to_argb() {
        let icons = super::logo_icons();
        let sizes: Vec<_> = icons.iter().map(|i| (i.width, i.height)).collect();
        assert_eq!(sizes, vec![(22, 22), (32, 32), (64, 64)]);
        for i in &icons {
            assert_eq!(i.data.len(), (i.width * i.height * 4) as usize);
        }
        // Top-left corner is transparent; the center is the dark "/" of the logo,
        // stored as A, R, G, B (the PNG pixel there is RGBA 32,32,33,251).
        let big = &icons[2];
        assert_eq!(big.data[0], 0);
        let c = ((32 * 64 + 32) * 4) as usize;
        assert_eq!(&big.data[c..c + 4], &[251, 32, 32, 33]);
    }
}
