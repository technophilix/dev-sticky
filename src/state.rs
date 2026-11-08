//! Shared application state: the store, open note windows and change listeners.

use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::db::Store;

pub struct AppState {
    pub store: Store,
    /// True while the top-bar icon is showing, so hidden windows can be brought back.
    pub tray_active: Cell<bool>,
    windows: RefCell<HashMap<String, glib::WeakRef<adw::ApplicationWindow>>>,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            tray_active: Cell::new(false),
            windows: RefCell::default(),
            listeners: RefCell::default(),
        }
    }

    /// Registers a callback run whenever any note is created, edited or deleted.
    pub fn on_notes_changed(&self, f: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Box::new(f));
    }

    pub fn notes_changed(&self) {
        for f in self.listeners.borrow().iter() {
            f();
        }
    }

    pub fn note_window(&self, id: &str) -> Option<adw::ApplicationWindow> {
        self.windows.borrow().get(id).and_then(|w| w.upgrade())
    }

    pub fn register_window(&self, id: &str, win: &adw::ApplicationWindow) {
        let mut map = self.windows.borrow_mut();
        map.retain(|_, w| w.upgrade().is_some());
        map.insert(id.to_string(), win.downgrade());
    }

    pub fn unregister_window(&self, id: &str) {
        self.windows.borrow_mut().remove(id);
    }
}
