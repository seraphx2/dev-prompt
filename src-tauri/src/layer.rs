//! Wayland: the overlay as a layer-shell surface.
//!
//! Wayland doesn't let an app place its own window — `center()` and
//! `set_position` are silently ignored, the compositor puts a window wherever
//! its policy says, and a window that grows keeps its top-left corner. Layer
//! shell (zwlr_layer_shell_v1) is the protocol launchers use instead: a layer
//! surface anchored to no edge is kept centred by the compositor at any size,
//! sits above normal windows, and never shows in the taskbar. KDE Plasma,
//! wlroots compositors (Sway, Hyprland, …) and Mir support it; GNOME doesn't.
//!
//! The library (gtk-layer-shell) is loaded at runtime rather than linked, so
//! it's an optional dependency: without it — or without a compositor that
//! speaks the protocol, or on X11 — the overlay stays a regular window,
//! exactly as before. The Flatpak bundles the library.

use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// `GtkLayerShellLayer::GTK_LAYER_SHELL_LAYER_OVERLAY` — above everything.
const LAYER_OVERLAY: c_int = 3;
/// `GtkLayerShellKeyboardMode::GTK_LAYER_SHELL_KEYBOARD_MODE_ON_DEMAND` — takes
/// keyboard focus like a normal window, and loses it when the user clicks
/// elsewhere, which is what lets focus loss dismiss the overlay. (`EXCLUSIVE`
/// would hold the keyboard for as long as it's shown.)
const KEYBOARD_ON_DEMAND: c_int = 2;

/// Whether the overlay is a layer surface (the compositor positions it).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Turn `window` into a centred overlay-layer surface. Must run before the
/// window is first shown (gtk-layer-shell has to take it over before GTK
/// realizes it). `Err` says why it stayed a regular window — never fatal.
pub fn init(window: &tauri::WebviewWindow) -> Result<(), String> {
    let gtk_window = window.gtk_window().map_err(|e| format!("no GTK window: {e}"))?;
    if !gtk_window.display().backend().is_wayland() {
        return Err("not a Wayland session".into());
    }
    if gtk_window.is_realized() {
        return Err("the window was already realized".into());
    }

    // SAFETY: a plain C library with no initialisers that matter here; kept
    // loaded for the life of the process (below), since its code runs every
    // time GTK maps the window.
    let lib = unsafe { libloading::Library::new("libgtk-layer-shell.so.0") }
        .map_err(|_| "gtk-layer-shell isn't installed".to_string())?;

    type Supported = unsafe extern "C" fn() -> c_int;
    type InitFor = unsafe extern "C" fn(*mut c_void);
    type SetInt = unsafe extern "C" fn(*mut c_void, c_int);
    type SetStr = unsafe extern "C" fn(*mut c_void, *const c_char);

    // A missing symbol is an error, not a crash. The function pointers are
    // copied out of the `Symbol`s; that's sound because the library is never
    // unloaded (forgotten below).
    fn sym<T: Copy>(lib: &libloading::Library, name: &str) -> Result<T, String> {
        let c = format!("{name}\0");
        // SAFETY: each `T` below matches its declaration in gtk-layer-shell.h
        // (>= 0.6, which has every one of these).
        unsafe { lib.get::<T>(c.as_bytes()) }
            .map(|s| *s)
            .map_err(|_| format!("gtk-layer-shell is too old (no {name})"))
    }
    let is_supported: Supported = sym(&lib, "gtk_layer_is_supported")?;
    let init_for_window: InitFor = sym(&lib, "gtk_layer_init_for_window")?;
    let set_layer: SetInt = sym(&lib, "gtk_layer_set_layer")?;
    let set_keyboard_mode: SetInt = sym(&lib, "gtk_layer_set_keyboard_mode")?;
    let set_namespace: SetStr = sym(&lib, "gtk_layer_set_namespace")?;

    // SAFETY: plain C calls on a live, not-yet-realized GtkWindow, in the
    // order gtk-layer-shell documents.
    unsafe {

        if is_supported() == 0 {
            return Err("the compositor doesn't support layer shell (GNOME, for one)".into());
        }

        let raw: *mut gtk::ffi::GtkWindow = gtk_window.upcast_ref::<gtk::Window>().to_glib_none().0;
        let raw = raw.cast::<c_void>();
        init_for_window(raw);
        set_layer(raw, LAYER_OVERLAY);
        set_keyboard_mode(raw, KEYBOARD_ON_DEMAND);
        // What the compositor knows it by (e.g. for KWin window rules).
        set_namespace(raw, c"dev-prompt".as_ptr());
        // No anchors and no margins: the compositor centres the surface, and
        // re-centres it whenever it changes size.
    }
    std::mem::forget(lib);
    ACTIVE.store(true, Ordering::Relaxed);
    Ok(())
}
