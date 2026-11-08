//! The notes list: search box plus a lazily loaded `ListView`.
//!
//! Rows are fetched from SQLite one page at a time as you scroll, and GTK only
//! creates widgets for the rows on screen, so this stays fast with many notes.
//! In step 2 this same view becomes the top-bar quick panel.

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::model::{self, NoteSummary};
use crate::state::AppState;

const PAGE_SIZE: usize = 50;
/// Start loading the next page when this close (in px) to the bottom.
const PREFETCH_PX: f64 = 400.0;

struct NoteList {
    state: Rc<AppState>,
    model: gio::ListStore,
    query: RefCell<String>,
    loaded: Cell<usize>,
    exhausted: Cell<bool>,
    content: gtk::Stack,
    empty_page: adw::StatusPage,
}

impl NoteList {
    fn fetch(&self, offset: usize, limit: usize) -> Vec<glib::BoxedAnyObject> {
        match self.state.store.list(&self.query.borrow(), offset, limit) {
            Ok(rows) => rows.into_iter().map(glib::BoxedAnyObject::new).collect(),
            Err(e) => {
                eprintln!("devsticky: listing notes failed: {e:#}");
                Vec::new()
            }
        }
    }

    fn load_more(&self) {
        if self.exhausted.get() {
            return;
        }
        let rows = self.fetch(self.loaded.get(), PAGE_SIZE);
        self.exhausted.set(rows.len() < PAGE_SIZE);
        self.loaded.set(self.loaded.get() + rows.len());
        self.model.extend_from_slice(&rows);
        self.update_empty_state();
    }

    /// Starts over for a new search query.
    fn reset(&self) {
        self.loaded.set(0);
        self.exhausted.set(false);
        self.model.remove_all();
        self.load_more();
    }

    /// Refreshes rows already loaded (after an edit) without losing the scroll position.
    fn refresh(&self) {
        let count = self.loaded.get().max(PAGE_SIZE);
        let rows = self.fetch(0, count);
        self.exhausted.set(rows.len() < count);
        self.loaded.set(rows.len());
        self.model.splice(0, self.model.n_items(), &rows);
        self.update_empty_state();
    }

    fn update_empty_state(&self) {
        if self.model.n_items() > 0 {
            self.content.set_visible_child_name("list");
            return;
        }
        if self.query.borrow().trim().is_empty() {
            self.empty_page.set_icon_name(Some("document-edit-symbolic"));
            self.empty_page.set_title("No Notes Yet");
            self.empty_page
                .set_description(Some("Press Ctrl+N or the + button to create one."));
        } else {
            self.empty_page.set_icon_name(Some("edit-find-symbolic"));
            self.empty_page.set_title("No Results");
            self.empty_page.set_description(Some(
                "Try other words, or #tag to filter by tag.",
            ));
        }
        self.content.set_visible_child_name("empty");
    }
}

/// Pops up the Edit / Delete menu for the note shown in `item`, at (x, y) in `row`.
fn show_row_menu(item: &gtk::ListItem, row: &gtk::Box, x: f64, y: f64) {
    let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
        return;
    };
    let id = obj.borrow::<NoteSummary>().id.clone();
    let menu = gio::Menu::new();
    for (label, action) in [("Edit", "win.open-note"), ("Delete", "win.delete-note")] {
        let entry = gio::MenuItem::new(Some(label), None);
        entry.set_action_and_target_value(Some(action), Some(&id.to_variant()));
        menu.append_item(&entry);
    }
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(row);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // The popover is temporary: detach it once it closes.
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}

/// Asks before deleting note `id`, closing its window if it is open.
fn confirm_delete(window: &adw::ApplicationWindow, state: &Rc<AppState>, id: String) {
    let title = state
        .store
        .get(&id)
        .ok()
        .flatten()
        .map(|n| n.title)
        .unwrap_or_default();
    let dialog = adw::AlertDialog::new(
        Some("Delete Note?"),
        Some(&format!("“{title}” will be permanently removed.")),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let state = state.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        if let Some(w) = state.note_window(&id) {
            ActionGroupExt::activate_action(&w, "discard", None);
        }
        if let Err(e) = state.store.delete(&id) {
            eprintln!("devsticky: delete failed: {e:#}");
        }
        state.notes_changed();
    });
    dialog.present(Some(window));
}

fn row_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();

        let dot = gtk::Box::builder().valign(gtk::Align::Start).margin_top(5).build();
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("heading");
        let pin = gtk::Image::from_icon_name("view-pin-symbolic");
        pin.add_css_class("dim-label");
        let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        title_row.append(&title);
        title_row.append(&pin);

        let snippet = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        snippet.add_css_class("dim-label");
        let meta = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        meta.add_css_class("caption");
        meta.add_css_class("note-meta");

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(&title_row);
        text.append(&snippet);
        text.append(&meta);

        let row = gtk::Box::builder()
            .spacing(12)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(6)
            .margin_end(6)
            .build();
        row.append(&dot);
        row.append(&text);
        item.set_child(Some(&row));

        // Right-click (or long-press on touch): Edit / Delete for this note.
        let click = gtk::GestureClick::builder().button(gtk::gdk::BUTTON_SECONDARY).build();
        let (li, r) = (item.downgrade(), row.clone());
        click.connect_pressed(move |g, _, x, y| {
            if let Some(item) = li.upgrade() {
                g.set_state(gtk::EventSequenceState::Claimed);
                show_row_menu(&item, &r, x, y);
            }
        });
        row.add_controller(click);
        let press = gtk::GestureLongPress::new();
        press.set_touch_only(true);
        let (li, r) = (item.downgrade(), row.clone());
        press.connect_pressed(move |_, x, y| {
            if let Some(item) = li.upgrade() {
                show_row_menu(&item, &r, x, y);
            }
        });
        row.add_controller(press);

        // Rebind whenever GTK recycles this row widget for a different note.
        item.connect_item_notify(move |item| {
            let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let n = obj.borrow::<NoteSummary>();
            dot.set_css_classes(&["color-dot", &format!("color-{}", n.color)]);
            title.set_label(&n.title);
            pin.set_visible(n.pinned);
            snippet.set_label(&n.snippet);
            snippet.set_visible(!n.snippet.is_empty());
            let mut m: Vec<String> = Vec::new();
            if n.tasks.1 > 0 {
                m.push(format!("☑ {}/{} done", n.tasks.0, n.tasks.1));
            }
            m.extend(n.tags.iter().map(|t| format!("#{t}")));
            m.push(model::relative_time(n.updated_at, crate::db::now()));
            meta.set_label(&m.join("  "));
        });
    });
    factory
}

/// The notes list window. With the tray icon running it acts as a quick panel:
/// shown from the top bar, hidden with Escape or after opening a note.
#[derive(Clone)]
pub struct Panel {
    pub window: adw::ApplicationWindow,
    search: gtk::SearchEntry,
    toasts: adw::ToastOverlay,
}

impl Panel {
    /// Shows the panel with the search box focused and its text selected.
    /// `token` is the Wayland activation token that allows taking focus.
    pub fn show(&self, token: Option<&str>) {
        if let Some(t) = token {
            self.window.set_startup_id(t);
        }
        self.window.present();
        self.search.grab_focus();
        self.search.select_region(0, -1);
    }

    /// Tray click: hide if shown, otherwise show. (Not based on focus: clicking the
    /// top bar can itself make the window inactive, so a second click would never hide.)
    pub fn toast(&self, msg: &str) {
        self.toasts.add_toast(adw::Toast::new(msg));
    }

    pub fn toggle(&self, token: Option<&str>) {
        if self.window.is_visible() {
            self.window.set_visible(false);
        } else {
            self.show(token);
        }
    }
}

pub fn build(app: &adw::Application, state: &Rc<AppState>) -> Panel {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search notes, or #tag")
        .hexpand(true)
        .build();
    let search_bar = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    search_bar.append(&search);

    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    let selection = gtk::NoSelection::new(Some(model.clone()));
    let list_view = gtk::ListView::builder()
        .model(&selection)
        .factory(&row_factory())
        .single_click_activate(true)
        .build();
    list_view.add_css_class("navigation-sidebar");

    let scroller = gtk::ScrolledWindow::builder()
        .child(&list_view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let empty_page = adw::StatusPage::new();
    let content = gtk::Stack::new();
    content.add_named(&scroller, Some("list"));
    content.add_named(&empty_page, Some("empty"));

    let header = adw::HeaderBar::new();
    let new_menu = gio::Menu::new();
    new_menu.append(Some("New Note"), Some("app.new-note"));
    new_menu.append(Some("New To-do List"), Some("app.new-todo"));
    let new_btn = adw::SplitButton::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("New Note (Ctrl+N)")
        .dropdown_tooltip("More: New To-do List (Ctrl+Shift+N)")
        .action_name("app.new-note")
        .menu_model(&new_menu)
        .build();
    header.pack_start(&new_btn);
    let menu = gio::Menu::new();
    let data = gio::Menu::new();
    data.append(Some("Export Notes…"), Some("app.export"));
    data.append(Some("Import Notes…"), Some("app.import"));
    menu.append_section(None, &data);
    let app_section = gio::Menu::new();
    app_section.append(Some("Start at Login"), Some("app.autostart"));
    app_section.append(Some("About DevSticky"), Some("app.about"));
    app_section.append(Some("Quit"), Some("app.quit"));
    menu.append_section(None, &app_section);
    header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main Menu")
            .build(),
    );

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&search_bar);
    toolbar.set_content(Some(&content));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("DevSticky")
        .default_width(400)
        .default_height(640)
        .content(&toasts)
        .build();
    // Typing anywhere in the window goes to the search box.
    search.set_key_capture_widget(Some(&window));

    let list = Rc::new(NoteList {
        state: state.clone(),
        model,
        query: RefCell::default(),
        loaded: Cell::new(0),
        exhausted: Cell::new(false),
        content,
        empty_page,
    });
    list.load_more();

    let weak = Rc::downgrade(&list);
    let adj = scroller.vadjustment();
    adj.connect_value_changed(move |adj| {
        if let Some(list) = weak.upgrade()
            && adj.value() + adj.page_size() >= adj.upper() - PREFETCH_PX
        {
            list.load_more();
        }
    });

    let weak = Rc::downgrade(&list);
    let adj = scroller.vadjustment();
    search.connect_search_changed(move |entry| {
        if let Some(list) = weak.upgrade() {
            *list.query.borrow_mut() = entry.text().to_string();
            list.reset();
            adj.set_value(0.0);
        }
    });

    let weak = Rc::downgrade(&list);
    let app_weak = app.downgrade();
    list_view.connect_activate(move |lv, pos| {
        let (Some(list), Some(app)) = (weak.upgrade(), app_weak.upgrade()) else {
            return;
        };
        if let Some(obj) = list.model.item(pos).and_downcast::<glib::BoxedAnyObject>() {
            let id = obj.borrow::<NoteSummary>().id.clone();
            crate::note_window::open(&app, &list.state, &id, None);
            // As a quick panel, get out of the way once a note is picked.
            if list.state.tray_active.get()
                && let Some(win) = lv.root().and_downcast::<gtk::Window>()
            {
                win.set_visible(false);
            }
        }
    });

    // Enter in the search box opens the first result.
    let lv = list_view.clone();
    let weak = Rc::downgrade(&list);
    search.connect_activate(move |_| {
        if weak.upgrade().is_some_and(|l| l.model.n_items() > 0) {
            lv.emit_by_name::<()>("activate", &[&0u32]);
        }
    });

    // Down arrow moves from the search box into the results.
    let keys = gtk::EventControllerKey::new();
    let lv = list_view.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Down {
            lv.scroll_to(0, gtk::ListScrollFlags::FOCUS, None);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    search.add_controller(keys);

    // Escape clears the search, then hides the panel (only when the tray icon
    // can bring it back; otherwise hiding the last window would quit).
    let st = state.clone();
    search.connect_stop_search(move |entry| {
        if !entry.text().is_empty() {
            entry.set_text("");
        } else if st.tray_active.get()
            && let Some(win) = entry.root().and_downcast::<gtk::Window>()
        {
            win.set_visible(false);
        }
    });
    let st = state.clone();
    let esc = gtk::ShortcutController::new();
    esc.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(move |w, _| {
            if st.tray_active.get()
                && let Some(win) = w.root().and_downcast::<gtk::Window>()
            {
                win.set_visible(false);
            }
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(esc);

    // Targets of the right-click menu.
    let open_note = gio::SimpleAction::new("open-note", Some(glib::VariantTy::STRING));
    let (st, app_weak) = (state.clone(), app.downgrade());
    open_note.connect_activate(move |_, param| {
        if let (Some(app), Some(id)) = (app_weak.upgrade(), param.and_then(|p| p.get::<String>())) {
            crate::note_window::open(&app, &st, &id, None);
        }
    });
    window.add_action(&open_note);
    let delete_note = gio::SimpleAction::new("delete-note", Some(glib::VariantTy::STRING));
    let (st, w) = (state.clone(), window.downgrade());
    delete_note.connect_activate(move |_, param| {
        if let (Some(w), Some(id)) = (w.upgrade(), param.and_then(|p| p.get::<String>())) {
            confirm_delete(&w, &st, id);
        }
    });
    window.add_action(&delete_note);

    // Ctrl+F focuses search.
    let focus = gio::SimpleAction::new("focus-search", None);
    let s = search.clone();
    focus.connect_activate(move |_, _| {
        s.grab_focus();
    });
    window.add_action(&focus);

    let weak = Rc::downgrade(&list);
    state.on_notes_changed(move || {
        if let Some(list) = weak.upgrade() {
            list.refresh();
        }
    });

    // The window owns the list controller; every other closure holds a weak ref.
    window.connect_destroy(move |_| {
        let _owner = &list;
    });

    Panel {
        window,
        search,
        toasts,
    }
}
