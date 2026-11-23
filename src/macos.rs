//! Small AppKit helpers for running as a menu-bar-only app.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn app() -> Option<objc2::rc::Retained<NSApplication>> {
    MainThreadMarker::new().map(NSApplication::sharedApplication)
}

/// No Dock icon and no app menu in the menu bar; only the status item stays.
///
/// `LSUIElement` in Info.plist should already do this, but GTK turns the process
/// into a regular foreground app when it opens the display (and a plain binary
/// started from a terminal has no Info.plist), so set it again after GTK starts.
pub fn hide_from_dock() {
    if let Some(app) = app() {
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

/// Makes DevSticky the active app, so a window shown from the menu bar gets
/// keyboard focus right away instead of needing an extra click.
pub fn activate() {
    if let Some(app) = app() {
        app.activate();
    }
}

/// Hands focus back to the app the user was in before.
pub fn deactivate() {
    if let Some(app) = app() {
        app.deactivate();
    }
}

/// Turns off GTK's (and libadwaita's) animations.
///
/// On macOS, GTK doesn't redraw fading overlays cleanly: when a message (toast)
/// or dialog faded out, the panel showed stripes or went see-through until it
/// was redrawn. Without animations they appear and disappear instantly.
pub fn disable_animations() {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
}
