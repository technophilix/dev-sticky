mod autostart;
mod db;
mod list_window;
mod markdown;
mod migrate;
mod model;
mod note_window;
mod state;
mod transfer;
mod tray;

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

use state::AppState;

pub const APP_ID: &str = "io.github.technophilix.DevSticky";

const CSS: &str = r#"
.color-dot { min-width: 12px; min-height: 12px; border-radius: 6px; }
.color-dot.color-yellow { background-color: #f6d32d; }
.color-dot.color-green  { background-color: #57e389; }
.color-dot.color-blue   { background-color: #62a0ea; }
.color-dot.color-purple { background-color: #c061cb; }
.color-dot.color-pink   { background-color: #f66151; }
.color-dot.color-orange { background-color: #ffa348; }

.note-meta { color: alpha(currentColor, 0.65); }

/* Tint the whole note window; the editor text area stays transparent over it. */
window.note-yellow toolbarview.note-root { background-color: alpha(#f6d32d, 0.16); }
window.note-green  toolbarview.note-root { background-color: alpha(#57e389, 0.14); }
window.note-blue   toolbarview.note-root { background-color: alpha(#62a0ea, 0.16); }
window.note-purple toolbarview.note-root { background-color: alpha(#c061cb, 0.14); }
window.note-pink   toolbarview.note-root { background-color: alpha(#f66151, 0.13); }
window.note-orange toolbarview.note-root { background-color: alpha(#ffa348, 0.15); }
textview.note-editor, textview.note-editor text { background-color: transparent; }

textview.code-view, textview.code-view text { background-color: transparent; }
.md-quote { color: alpha(currentColor, 0.7); }
.task-done { text-decoration-line: line-through; color: alpha(currentColor, 0.55); }
"#;

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("no display"),
        &provider,
        // Above GtkSourceView's style-scheme CSS so note colors show through.
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
}

fn setup_actions(app: &adw::Application, state: &Rc<AppState>, panel: &list_window::Panel) {
    let new_note = gio::SimpleAction::new("new-note", None);
    let (a, s) = (app.downgrade(), state.clone());
    new_note.connect_activate(move |_, _| {
        if let Some(app) = a.upgrade() {
            note_window::create(&app, &s, None, "");
        }
    });
    app.add_action(&new_note);

    let new_todo = gio::SimpleAction::new("new-todo", None);
    let (a, s) = (app.downgrade(), state.clone());
    new_todo.connect_activate(move |_, _| {
        if let Some(app) = a.upgrade() {
            note_window::create(&app, &s, None, model::TODO_TEMPLATE);
        }
    });
    app.add_action(&new_todo);

    let show = gio::SimpleAction::new("show-notes", None);
    let p = panel.clone();
    show.connect_activate(move |_, _| p.show(None));
    app.add_action(&show);

    let autostart = gio::SimpleAction::new_stateful(
        "autostart",
        None,
        &autostart::is_enabled().to_variant(),
    );
    autostart.connect_activate(|action, _| {
        let enable = !action.state().and_then(|v| v.get::<bool>()).unwrap_or(false);
        match autostart::set_enabled(enable) {
            Ok(()) => action.set_state(&enable.to_variant()),
            Err(e) => eprintln!("devsticky: changing autostart failed: {e:#}"),
        }
    });
    app.add_action(&autostart);

    let export = gio::SimpleAction::new("export", None);
    let (s, p) = (state.clone(), panel.clone());
    export.connect_activate(move |_, _| export_notes(&s, &p));
    app.add_action(&export);

    let import = gio::SimpleAction::new("import", None);
    let (s, p) = (state.clone(), panel.clone());
    import.connect_activate(move |_, _| import_notes(&s, &p));
    app.add_action(&import);

    let about = gio::SimpleAction::new("about", None);
    let w = panel.window.downgrade();
    about.connect_activate(move |_, _| {
        let dialog = adw::AboutDialog::builder()
            .application_name("DevSticky")
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .comments("Sticky notes for developers: Markdown, code snippets, tags and search.")
            .license_type(gtk::License::MitX11)
            .build();
        dialog.present(w.upgrade().as_ref());
    });
    app.add_action(&about);

    let quit = gio::SimpleAction::new("quit", None);
    let a = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = a.upgrade() {
            // Closing windows triggers their autosave.
            for w in app.windows() {
                w.close();
            }
            app.quit();
        }
    });
    app.add_action(&quit);

    app.set_accels_for_action("app.new-note", &["<Ctrl>n"]);
    app.set_accels_for_action("app.new-todo", &["<Ctrl><Shift>n"]);
    app.set_accels_for_action("win.toggle-task", &["<Ctrl>t"]);
    app.set_accels_for_action("app.show-notes", &["<Ctrl>l"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
    app.set_accels_for_action("window.close", &["<Ctrl>w"]);
    app.set_accels_for_action("win.focus-search", &["<Ctrl>f"]);
    app.set_accels_for_action("win.toggle-preview", &["<Ctrl>e"]);
    app.set_accels_for_action("win.copy-all", &["<Ctrl><Shift>c"]);
}

fn json_filter() -> gio::ListStore {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("DevSticky export (*.json)"));
    filter.add_pattern("*.json");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    filters
}

/// Saves edits still waiting in open note windows, so export/import see them.
fn flush_open_notes(app: Option<gtk::Application>) {
    for w in app.map(|a| a.windows()).unwrap_or_default() {
        // Only note windows have this action; others ignore the call.
        if let Some(w) = w.downcast_ref::<adw::ApplicationWindow>()
            && w.lookup_action("save-now").is_some()
        {
            ActionGroupExt::activate_action(w, "save-now", None);
        }
    }
}

fn export_notes(state: &Rc<AppState>, panel: &list_window::Panel) {
    panel.show(None);
    let dialog = gtk::FileDialog::builder()
        .title("Export Notes")
        .initial_name(transfer::default_file_name())
        .filters(&json_filter())
        .build();
    let (state, panel) = (state.clone(), panel.clone());
    dialog.save(Some(&panel.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return }; // cancelled
        let Some(path) = file.path() else {
            panel.toast("Can only export to a local file");
            return;
        };
        flush_open_notes(panel.window.application());
        let result = transfer::export(&state.store)
            .and_then(|(json, n)| std::fs::write(&path, json).map(|_| n).map_err(Into::into));
        match result {
            Ok(n) => panel.toast(&format!("Exported {n} notes")),
            Err(e) => {
                eprintln!("devsticky: export failed: {e:#}");
                panel.toast(&format!("Export failed: {e}"));
            }
        }
    });
}

fn import_notes(state: &Rc<AppState>, panel: &list_window::Panel) {
    panel.show(None);
    let dialog = gtk::FileDialog::builder()
        .title("Import Notes")
        .filters(&json_filter())
        .build();
    let (state, panel) = (state.clone(), panel.clone());
    dialog.open(Some(&panel.window.clone()), gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return }; // cancelled
        let Some(path) = file.path() else {
            panel.toast("Can only import from a local file");
            return;
        };
        flush_open_notes(panel.window.application());
        let result = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|json| transfer::import(&state.store, &json));
        match result {
            Ok(report) => {
                for id in &report.updated_ids {
                    if let Some(w) = state.note_window(id) {
                        ActionGroupExt::activate_action(&w, "reload", None);
                    }
                }
                state.notes_changed();
                panel.toast(&report.summary());
            }
            Err(e) => {
                eprintln!("devsticky: import failed: {e:#}");
                panel.toast(&format!("Import failed: {e}"));
            }
        }
    });
}

/// Without a tray icon, quits once every window has been closed or hidden.
/// With one, the app keeps running in the top bar.
pub fn quit_if_no_visible_windows(app: &gtk::Application, state: &Rc<AppState>) {
    let app = app.clone();
    let state = state.clone();
    glib::idle_add_local_once(move || {
        if !state.tray_active.get() && !app.windows().iter().any(|w| w.is_visible()) {
            app.quit();
        }
    });
}

/// Starts the top-bar icon and handles its clicks on the GTK main loop.
fn start_tray(app: &adw::Application, state: &Rc<AppState>, panel: &list_window::Panel) {
    let (tx, rx) = async_channel::unbounded();
    let handle = match tray::spawn(tx) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("devsticky: top-bar icon unavailable ({e}); running as a normal window");
            return;
        }
    };
    state.tray_active.set(true);

    let app = app.clone();
    let state = state.clone();
    let panel = panel.clone();
    glib::spawn_future_local(async move {
        // Keeps the app running with no windows open while the icon is shown.
        let mut hold = Some(app.hold());
        let _handle = handle;
        while let Ok(event) = rx.recv().await {
            match event {
                tray::TrayEvent::Toggle(token) => panel.toggle(token.as_deref()),
                tray::TrayEvent::Show(token) => panel.show(token.as_deref()),
                tray::TrayEvent::NewNote(token) => {
                    note_window::create(&app, &state, token.as_deref(), "")
                }
                tray::TrayEvent::NewTodo(token) => {
                    note_window::create(&app, &state, token.as_deref(), model::TODO_TEMPLATE)
                }
                tray::TrayEvent::Quit => app.activate_action("quit", None),
                tray::TrayEvent::Gone => {
                    // No tray anymore: make sure the user can still reach the app.
                    state.tray_active.set(false);
                    hold.take();
                    if !app.windows().iter().any(|w| w.is_visible()) {
                        panel.show(None);
                    }
                }
                tray::TrayEvent::Back => {
                    state.tray_active.set(true);
                    if hold.is_none() {
                        hold = Some(app.hold());
                    }
                }
            }
        }
    });
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        // Arguments are forwarded to the already-running instance, if any.
        // CAN_OVERRIDE_APP_ID allows `--gapplication-app-id=…` to run a separate test copy.
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::CAN_OVERRIDE_APP_ID)
        .build();
    let panel: Rc<RefCell<Option<list_window::Panel>>> = Rc::default();
    let state_cell: Rc<RefCell<Option<Rc<AppState>>>> = Rc::default();

    let (p, sc) = (panel.clone(), state_cell.clone());
    app.connect_startup(move |app| {
        load_css();
        let data_dir = glib::user_data_dir().join("devsticky");
        migrate::run(&data_dir);
        let path = data_dir.join("notes.db");
        let store = match db::Store::open(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("devsticky: {e:#}");
                std::process::exit(1);
            }
        };
        let state = Rc::new(AppState::new(store));
        let list = list_window::build(app, &state);
        setup_actions(app, &state, &list);
        // Closing the list only hides it; open notes stay, and the tray can bring it back.
        list.window.set_hide_on_close(true);
        let st = state.clone();
        list.window.connect_hide(move |w| {
            if let Some(app) = w.application() {
                quit_if_no_visible_windows(&app, &st);
            }
        });
        start_tray(app, &state, &list);
        *p.borrow_mut() = Some(list);
        *sc.borrow_mut() = Some(state);
    });

    // Runs in the primary instance for every launch, including later ones.
    //   devsticky               show the notes panel
    //   devsticky --new-note    create a note
    //   devsticky --new-todo    create a to-do list
    //   devsticky --background  first launch only: start with just the top-bar icon
    let first = std::cell::Cell::new(true);
    app.connect_command_line(move |app, cmd| {
        let args: Vec<String> = cmd
            .arguments()
            .iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let is_first = first.replace(false);
        let Some(state) = state_cell.borrow().clone() else {
            return glib::ExitCode::FAILURE;
        };
        if args.iter().any(|a| a == "--new-todo") {
            note_window::create(app, &state, None, model::TODO_TEMPLATE);
        } else if args.iter().any(|a| a == "--new-note") {
            note_window::create(app, &state, None, "");
        } else if !(is_first && args.iter().any(|a| a == "--background") && state.tray_active.get())
            && let Some(p) = panel.borrow().as_ref()
        {
            p.show(None);
        }
        glib::ExitCode::SUCCESS
    });

    app.run()
}
