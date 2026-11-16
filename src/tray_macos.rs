//! Menu-bar icon on macOS (NSStatusItem, via the `tray-icon` crate).
//!
//! Left click toggles the quick panel; right click opens the menu. Unlike the
//! Linux tray, everything runs on the GTK main thread: AppKit calls the event
//! handlers from the main run loop, and they forward [`TrayEvent`]s through the
//! same channel the Linux tray uses.

use gtk::prelude::*;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Same events as the Linux tray. macOS has no activation tokens, so the
/// token is always `None`, and the menu bar never goes away (no `Gone`/`Back`).
#[derive(Debug)]
#[allow(dead_code)]
pub enum TrayEvent {
    Toggle(Option<String>),
    Show(Option<String>),
    NewNote(Option<String>),
    NewTodo(Option<String>),
    Quit,
    Gone,
    Back,
}

pub struct TrayHandle {
    _icon: TrayIcon,
    // The menu must outlive the status item.
    _menu: Menu,
}

/// Adds the DevSticky icon to the menu bar. Must run on the main thread.
pub fn spawn(tx: async_channel::Sender<TrayEvent>) -> anyhow::Result<TrayHandle> {
    let search = MenuItem::new("Search Notes", true, None);
    let new_note = MenuItem::new("New Note", true, None);
    let new_todo = MenuItem::new("New To-do List", true, None);
    let quit = MenuItem::new("Quit DevSticky", true, None);
    let menu = Menu::with_items(&[
        &search,
        &new_note,
        &new_todo,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;

    let ids = [
        (search.id().clone(), TrayEvent::Show as fn(Option<String>) -> TrayEvent),
        (new_note.id().clone(), TrayEvent::NewNote),
        (new_todo.id().clone(), TrayEvent::NewTodo),
    ];
    let quit_id = quit.id().clone();
    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let msg = if event.id == quit_id {
            TrayEvent::Quit
        } else if let Some((_, make)) = ids.iter().find(|(id, _)| *id == event.id) {
            make(None)
        } else {
            return;
        };
        let _ = menu_tx.try_send(msg);
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = tx.try_send(TrayEvent::Toggle(None));
        }
    }));

    let mut builder = TrayIconBuilder::new()
        .with_tooltip("DevSticky — click to search your notes")
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(false);
    if let Some(icon) = logo_icon() {
        builder = builder.with_icon(icon);
    } else {
        builder = builder.with_title("DevSticky");
    }
    Ok(TrayHandle {
        _icon: builder.build()?,
        _menu: menu,
    })
}

/// The app logo, embedded in the binary. AppKit scales it to the menu-bar height.
const ICON_PNG: &[u8] =
    include_bytes!("../data/icons/hicolor/64x64/apps/io.github.technophilix.DevSticky.png");

/// Decodes the embedded logo into straight (non-premultiplied) RGBA.
fn logo_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let texture = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from_static(ICON_PNG)).ok()?;
    let (w, h) = (texture.width(), texture.height());
    let mut dl = gtk::gdk::TextureDownloader::new(&texture);
    dl.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = dl.download_bytes();
    let row = w as usize * 4;
    let data = bytes
        .chunks(stride)
        .take(h as usize)
        .flat_map(|r| r[..row].iter().copied())
        .collect();
    Some((data, w as u32, h as u32))
}

fn logo_icon() -> Option<tray_icon::Icon> {
    let (data, w, h) = logo_rgba()?;
    tray_icon::Icon::from_rgba(data, w, h).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn logo_png_decodes_to_rgba() {
        let (data, w, h) = super::logo_rgba().expect("logo decodes");
        assert_eq!((w, h), (64, 64));
        assert_eq!(data.len(), (w * h * 4) as usize);
    }
}
