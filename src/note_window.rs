//! A single note: Markdown editor, rendered preview, tags, color and pin.
//! Changes are autosaved shortly after you stop typing and when the window closes.

use adw::prelude::*;
use gtk::{gio, glib};
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::markdown;
use crate::model::{self, Note};
use crate::state::AppState;

const SAVE_DELAY: Duration = Duration::from_millis(500);

struct Editor {
    state: Rc<AppState>,
    window: adw::ApplicationWindow,
    note: RefCell<Note>,
    buffer: sourceview5::Buffer,
    tags_entry: gtk::Entry,
    stack: adw::ViewStack,
    preview: gtk::Box,
    toasts: adw::ToastOverlay,
    pending_save: RefCell<Option<glib::SourceId>>,
    deleted: Cell<bool>,
    /// For a just-created note: its starting text. If it is closed unchanged
    /// (still empty, or still just the to-do template) it is discarded.
    discard_if_unchanged: RefCell<Option<String>>,
    /// Set while replacing the editor contents from the store, so that isn't saved back.
    loading: Cell<bool>,
}

impl Editor {
    fn current_body(&self) -> String {
        let (start, end) = self.buffer.bounds();
        self.buffer.text(&start, &end, false).to_string()
    }

    fn toast(&self, msg: &str) {
        let t = adw::Toast::new(msg);
        t.set_timeout(2);
        self.toasts.add_toast(t);
    }

    fn copy(&self, text: &str) {
        self.window.clipboard().set_text(text);
        self.toast("Copied to clipboard");
    }

    fn schedule_save(self: &Rc<Self>) {
        if self.loading.get() {
            return;
        }
        if let Some(id) = self.pending_save.take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(SAVE_DELAY, move || {
            if let Some(ed) = weak.upgrade() {
                // This source has fired; forget its id without removing it.
                ed.pending_save.take();
                ed.save();
            }
        });
        *self.pending_save.borrow_mut() = Some(id);
    }

    fn flush(&self) {
        if let Some(id) = self.pending_save.take() {
            id.remove();
            self.save();
        }
    }

    fn save(&self) {
        if self.deleted.get() {
            return;
        }
        let body = self.current_body();
        let tags = model::parse_tags(&self.tags_entry.text());
        {
            let mut note = self.note.borrow_mut();
            if note.body == body && note.tags == tags {
                return;
            }
            note.body = body;
            note.tags = tags;
        }
        self.write();
    }

    /// Persists the in-memory note and tells the rest of the app.
    fn write(&self) {
        let mut note = self.note.borrow_mut();
        if let Err(e) = self.state.store.save(&mut note) {
            eprintln!("devsticky: saving note failed: {e:#}");
            drop(note);
            self.toast("Could not save note");
            return;
        }
        self.window.set_title(Some(&note.title));
        drop(note);
        self.state.notes_changed();
    }

    fn set_color(&self, color: &str) {
        let color = model::normalize_color(color);
        let old = std::mem::replace(&mut self.note.borrow_mut().color, color.to_string());
        self.window.remove_css_class(&format!("note-{old}"));
        self.window.add_css_class(&format!("note-{color}"));
        if old != color {
            self.write();
        }
    }

    fn render_preview(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let on_copy: markdown::CopyFn = Rc::new(move |text| {
            if let Some(ed) = weak.upgrade() {
                ed.copy(text);
            }
        });
        let weak = Rc::downgrade(self);
        let on_task: markdown::TaskFn = Rc::new(move |offset, done| {
            if let Some(ed) = weak.upgrade() {
                ed.set_task(offset, done);
            }
        });
        markdown::render_into(&self.preview, &self.current_body(), on_copy, on_task);
    }

    /// A preview checkbox was clicked: write "[x]" / "[ ]" back into the text.
    /// Same-length edit, so the other checkboxes' offsets stay valid.
    fn set_task(&self, offset: usize, done: bool) {
        let body = self.current_body();
        let Some(pos) = markdown::task_state_char_offset(&body, offset) else {
            return;
        };
        let mut start = self.buffer.iter_at_offset(pos as i32);
        let mut end = start;
        end.forward_char();
        self.buffer.delete(&mut start, &mut end);
        self.buffer.insert(&mut start, if done { "x" } else { " " });
    }

    /// Ctrl+T: tick/untick the checkbox on the cursor's line, or make the line one.
    fn toggle_task_at_cursor(&self) {
        let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let mut start = cursor;
        start.set_line_offset(0);
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let line = self.buffer.text(&start, &end, false).to_string();
        let new = model::toggle_task_line(&line);
        let line_no = start.line();
        self.buffer.begin_user_action();
        self.buffer.delete(&mut start, &mut end);
        self.buffer.insert(&mut start, &new);
        self.buffer.end_user_action();
        // Leave the cursor at the end of the edited line.
        if let Some(mut it) = self.buffer.iter_at_line(line_no) {
            if !it.ends_line() {
                it.forward_to_line_end();
            }
            self.buffer.place_cursor(&it);
        }
    }

    /// Enter in a list: start the next item ("- [ ] " for to-dos), or end the
    /// list if the current item is empty. Returns false to let Enter work normally.
    fn handle_enter(&self) -> bool {
        if self.buffer.has_selection() {
            return false;
        }
        let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let mut line_start = cursor;
        line_start.set_line_offset(0);
        let before = self.buffer.text(&line_start, &cursor, false).to_string();
        match model::list_enter(&before) {
            Some(model::ListEnter::Continue(prefix)) => {
                let mut at = cursor;
                self.buffer.begin_user_action();
                self.buffer.insert(&mut at, &format!("\n{prefix}"));
                self.buffer.end_user_action();
                true
            }
            Some(model::ListEnter::EndList) => {
                let (mut a, mut b) = (line_start, cursor);
                self.buffer.delete(&mut a, &mut b);
                true
            }
            None => false,
        }
    }

    fn confirm_delete(self: &Rc<Self>) {
        let dialog = adw::AlertDialog::new(
            Some("Delete Note?"),
            Some("This note will be permanently removed."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            let Some(ed) = weak.upgrade() else { return };
            if response != "delete" {
                return;
            }
            if let Some(id) = ed.pending_save.take() {
                id.remove();
            }
            let id = ed.note.borrow().id.clone();
            match ed.state.store.delete(&id) {
                Ok(()) => {
                    ed.deleted.set(true);
                    ed.state.notes_changed();
                    ed.window.close();
                }
                Err(e) => {
                    eprintln!("devsticky: delete failed: {e:#}");
                    ed.toast("Could not delete note");
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Replaces the editor contents with the stored note (e.g. after an import).
    fn reload(self: &Rc<Self>) {
        if let Some(id) = self.pending_save.take() {
            id.remove();
        }
        let id = self.note.borrow().id.clone();
        let Ok(Some(note)) = self.state.store.get(&id) else {
            return;
        };
        self.loading.set(true);
        self.buffer.set_text(&note.body);
        self.tags_entry.set_text(&model::format_tags(&note.tags));
        self.loading.set(false);

        let old_color = self.note.borrow().color.clone();
        self.window.remove_css_class(&format!("note-{old_color}"));
        self.window.add_css_class(&format!("note-{}", note.color));
        if let Some(a) = self.window.lookup_action("color").and_downcast::<gio::SimpleAction>() {
            a.set_state(&note.color.to_variant());
        }
        if let Some(a) = self.window.lookup_action("pin").and_downcast::<gio::SimpleAction>() {
            a.set_state(&note.pinned.to_variant());
        }
        self.window.set_title(Some(&note.title));
        self.discard_if_unchanged.replace(None);
        *self.note.borrow_mut() = note;
        if self.stack.visible_child_name().as_deref() == Some("preview") {
            self.render_preview();
        }
        self.toast("Updated from import");
    }

    fn update_style_scheme(&self) {
        self.buffer.set_style_scheme(markdown::style_scheme().as_ref());
    }
}

fn build_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let colors = gio::Menu::new();
    for (id, label) in model::COLORS {
        colors.append(Some(label), Some(&format!("win.color::{id}")));
    }
    menu.append_section(Some("Color"), &colors);
    let actions = gio::Menu::new();
    actions.append(Some("Pin to Top"), Some("win.pin"));
    actions.append(Some("Copy Note as Markdown"), Some("win.copy-all"));
    menu.append_section(None, &actions);
    let danger = gio::Menu::new();
    danger.append(Some("Delete Note"), Some("win.delete"));
    menu.append_section(None, &danger);
    menu
}

fn add_actions(ed: &Rc<Editor>) {
    let win = &ed.window;
    let note = ed.note.borrow();

    let color = gio::SimpleAction::new_stateful(
        "color",
        Some(glib::VariantTy::STRING),
        &note.color.to_variant(),
    );
    let weak = Rc::downgrade(ed);
    color.connect_activate(move |action, param| {
        let (Some(ed), Some(c)) = (weak.upgrade(), param.and_then(|p| p.get::<String>())) else {
            return;
        };
        action.set_state(&c.to_variant());
        ed.set_color(&c);
    });
    win.add_action(&color);

    let pin = gio::SimpleAction::new_stateful("pin", None, &note.pinned.to_variant());
    let weak = Rc::downgrade(ed);
    pin.connect_activate(move |action, _| {
        let Some(ed) = weak.upgrade() else { return };
        let pinned = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        action.set_state(&pinned.to_variant());
        ed.note.borrow_mut().pinned = pinned;
        ed.write();
    });
    win.add_action(&pin);

    let copy_all = gio::SimpleAction::new("copy-all", None);
    let weak = Rc::downgrade(ed);
    copy_all.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            ed.copy(&ed.current_body());
        }
    });
    win.add_action(&copy_all);

    let delete = gio::SimpleAction::new("delete", None);
    let weak = Rc::downgrade(ed);
    delete.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            ed.confirm_delete();
        }
    });
    win.add_action(&delete);

    let toggle = gio::SimpleAction::new("toggle-preview", None);
    let weak = Rc::downgrade(ed);
    toggle.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            let next = if ed.stack.visible_child_name().as_deref() == Some("edit") {
                "preview"
            } else {
                "edit"
            };
            ed.stack.set_visible_child_name(next);
        }
    });
    win.add_action(&toggle);

    // Used by export/import (see main.rs): save pending edits now / reload from the store.
    let save_now = gio::SimpleAction::new("save-now", None);
    let weak = Rc::downgrade(ed);
    save_now.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            ed.flush();
        }
    });
    win.add_action(&save_now);

    let reload = gio::SimpleAction::new("reload", None);
    let weak = Rc::downgrade(ed);
    reload.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            ed.reload();
        }
    });
    win.add_action(&reload);

    // The note was deleted from the list's right-click menu: close without saving.
    let discard = gio::SimpleAction::new("discard", None);
    let weak = Rc::downgrade(ed);
    discard.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            if let Some(id) = ed.pending_save.take() {
                id.remove();
            }
            ed.deleted.set(true);
            ed.window.close();
        }
    });
    win.add_action(&discard);

    let toggle_task = gio::SimpleAction::new("toggle-task", None);
    let weak = Rc::downgrade(ed);
    toggle_task.connect_activate(move |_, _| {
        if let Some(ed) = weak.upgrade() {
            if ed.stack.visible_child_name().as_deref() != Some("edit") {
                ed.stack.set_visible_child_name("edit");
            }
            ed.toggle_task_at_cursor();
        }
    });
    win.add_action(&toggle_task);
}

/// `new_from`: the starting text of a note that was just created (it opens in
/// edit mode and is discarded if closed unchanged); None for existing notes.
fn build(
    app: &adw::Application,
    state: &Rc<AppState>,
    note: Note,
    new_from: Option<String>,
) -> Rc<Editor> {
    // Editor
    let buffer = sourceview5::Buffer::new(None::<&gtk::TextTagTable>);
    if let Some(lang) = sourceview5::LanguageManager::default().language("markdown") {
        buffer.set_language(Some(&lang));
    }
    buffer.set_highlight_syntax(true);
    buffer.begin_irreversible_action();
    buffer.set_text(&note.body);
    buffer.end_irreversible_action();
    buffer.place_cursor(&buffer.end_iter());

    let view = sourceview5::View::builder()
        .buffer(&buffer)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .auto_indent(true)
        .insert_spaces_instead_of_tabs(true)
        .tab_width(4)
        .indent_on_tab(true)
        .smart_backspace(true)
        .top_margin(12)
        .bottom_margin(24)
        .left_margin(16)
        .right_margin(16)
        .build();
    view.add_css_class("note-editor");
    let edit_scroller = gtk::ScrolledWindow::builder().child(&view).build();

    // Preview
    let preview = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(16)
        .margin_bottom(24)
        .margin_start(18)
        .margin_end(18)
        .build();
    let preview_scroller = gtk::ScrolledWindow::builder()
        .child(&preview)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let stack = adw::ViewStack::new();
    stack.add_titled_with_icon(&edit_scroller, Some("edit"), "Edit", "document-edit-symbolic");
    stack.add_titled_with_icon(
        &preview_scroller,
        Some("preview"),
        "Preview",
        "view-reveal-symbolic",
    );
    // Existing notes open rendered; new and empty ones open ready for typing.
    let editing = new_from.is_some() || note.body.trim().is_empty();
    stack.set_visible_child_name(if editing { "edit" } else { "preview" });

    // Tags
    let tags_entry = gtk::Entry::builder()
        .placeholder_text("Tags: bug, rust, todo")
        .text(model::format_tags(&note.tags))
        .hexpand(true)
        .build();
    let tags_bar = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    tags_bar.append(&tags_entry);

    // Header
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(
        &adw::ViewSwitcher::builder()
            .stack(&stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build(),
    ));
    header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&build_menu())
            .tooltip_text("Note Menu")
            .build(),
    );
    header.pack_start(
        &gtk::Button::builder()
            .icon_name("object-select-symbolic")
            .tooltip_text("Checkbox: add or tick (Ctrl+T)")
            .action_name("win.toggle-task")
            .build(),
    );
    let copy_btn = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy Note (Ctrl+Shift+C)")
        .action_name("win.copy-all")
        .build();
    header.pack_end(&copy_btn);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_css_class("note-root");
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&tags_bar);
    toolbar.set_content(Some(&stack));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(&note.title)
        .default_width(560)
        .default_height(620)
        .content(&toasts)
        .build();
    window.add_css_class(&format!("note-{}", note.color));

    let ed = Rc::new(Editor {
        state: state.clone(),
        window: window.clone(),
        note: RefCell::new(note),
        buffer,
        tags_entry,
        stack,
        preview,
        toasts,
        pending_save: RefCell::default(),
        deleted: Cell::new(false),
        discard_if_unchanged: RefCell::new(new_from),
        loading: Cell::new(false),
    });
    ed.update_style_scheme();
    add_actions(&ed);
    if !editing {
        ed.render_preview();
        // Otherwise the first selectable preview label grabs focus and selects all its text.
        GtkWindowExt::set_focus(&window, Some(&copy_btn));
    } else {
        view.grab_focus();
    }

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&ed);
    keys.connect_key_pressed(move |_, key, _, mods| {
        let plain = !mods.intersects(
            gtk::gdk::ModifierType::SHIFT_MASK
                | gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::ALT_MASK,
        );
        if plain
            && matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
            && weak.upgrade().is_some_and(|ed| ed.handle_enter())
        {
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    view.add_controller(keys);

    let weak = Rc::downgrade(&ed);
    ed.buffer.connect_changed(move |_| {
        if let Some(ed) = weak.upgrade() {
            ed.schedule_save();
        }
    });
    let weak = Rc::downgrade(&ed);
    ed.tags_entry.connect_changed(move |_| {
        if let Some(ed) = weak.upgrade() {
            ed.schedule_save();
        }
    });
    let weak = Rc::downgrade(&ed);
    ed.stack.connect_visible_child_name_notify(move |stack| {
        if let Some(ed) = weak.upgrade() {
            if stack.visible_child_name().as_deref() == Some("preview") {
                ed.render_preview();
            } else {
                view.grab_focus();
            }
        }
    });
    let weak = Rc::downgrade(&ed);
    let dark_handler = adw::StyleManager::default().connect_dark_notify(move |_| {
        if let Some(ed) = weak.upgrade() {
            ed.update_style_scheme();
            if ed.stack.visible_child_name().as_deref() == Some("preview") {
                ed.render_preview();
            }
        }
    });

    // The close handler owns the editor (all other closures hold weak refs).
    let owner = ed.clone();
    let dark_handler = RefCell::new(Some(dark_handler));
    window.connect_close_request(move |_| {
        if let Some(h) = dark_handler.take() {
            adw::StyleManager::default().disconnect(h);
        }
        owner.flush();
        let id = owner.note.borrow().id.clone();
        owner.state.unregister_window(&id);
        let unchanged = owner.discard_if_unchanged.borrow().as_deref().is_some_and(|t| {
            owner.current_body().trim() == t.trim() && owner.tags_entry.text().trim().is_empty()
        });
        if unchanged && !owner.deleted.get() {
            // Never-used new note: drop it entirely instead of leaving it behind.
            let _ = owner.state.store.purge(&id);
            owner.state.notes_changed();
        }
        if let Some(app) = owner.window.application() {
            crate::quit_if_no_visible_windows(&app, &owner.state);
        }
        glib::Propagation::Proceed
    });

    ed
}

/// Opens (or focuses) the window for note `id`. `token` is a Wayland
/// activation token, needed when opening from the tray so the window gets focus.
pub fn open(app: &adw::Application, state: &Rc<AppState>, id: &str, token: Option<&str>) {
    open_note(app, state, id, token, None);
}

fn open_note(
    app: &adw::Application,
    state: &Rc<AppState>,
    id: &str,
    token: Option<&str>,
    new_from: Option<String>,
) {
    let present = |win: &adw::ApplicationWindow| {
        if let Some(t) = token {
            win.set_startup_id(t);
        }
        win.present();
    };
    if let Some(win) = state.note_window(id) {
        present(&win);
        return;
    }
    match state.store.get(id) {
        Ok(Some(note)) => {
            let ed = build(app, state, note, new_from);
            state.register_window(id, &ed.window);
            present(&ed.window);
        }
        Ok(None) => {}
        Err(e) => eprintln!("devsticky: loading note failed: {e:#}"),
    }
}

/// Creates a new note starting with `template` ("" for a blank note,
/// [`model::TODO_TEMPLATE`] for a to-do list) and opens it for editing.
pub fn create(app: &adw::Application, state: &Rc<AppState>, token: Option<&str>, template: &str) {
    let result = state.store.create().and_then(|mut note| {
        if !template.is_empty() {
            note.body = template.to_string();
            state.store.save(&mut note)?;
        }
        Ok(note)
    });
    match result {
        Ok(note) => {
            state.notes_changed();
            open_note(app, state, &note.id, token, Some(template.to_string()));
        }
        Err(e) => eprintln!("devsticky: creating note failed: {e:#}"),
    }
}
