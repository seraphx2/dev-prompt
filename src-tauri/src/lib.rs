mod actions;
mod cache;
mod commands;
mod config;
mod error;
mod index;
mod launch;
mod scan;

use tauri::{Emitter, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

use commands::AppState;

const OVERLAY_LABEL: &str = "overlay";

/// Show the overlay centered and focused, or hide it if it is already visible.
fn toggle_overlay(window: &WebviewWindow) {
    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => {
            let _ = window.center();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit("overlay:shown", ());
        }
    }
}

fn apply_overlay_effects(window: &WebviewWindow) {
    #[cfg(windows)]
    {
        use window_vibrancy::apply_acrylic;
        // Dark, semi-opaque acrylic behind the translucent panel.
        let _ = apply_acrylic(window, Some((18, 18, 22, 125)));
        // Clip the whole window (acrylic included) to a small rounded rect so the
        // corners outside the panel show the desktop, not the acrylic fill.
        round_window_corners(window);
    }
    // macOS (NSVisualEffect vibrancy) and Linux blur are wired up in a later
    // milestone; on those platforms the panel simply renders opaque for now.
    #[cfg(not(windows))]
    let _ = window;
}

/// Ask DWM (Windows 11) to round the actual window corners. `ROUNDSMALL` is the
/// tighter ~4px radius; `ROUND` would be the larger ~8px one.
#[cfg(windows)]
fn round_window_corners(window: &WebviewWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUNDSMALL: i32 = 3;

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: isize,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> i32;
    }

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let pref: i32 = DWMWCP_ROUNDSMALL;
    unsafe {
        DwmSetWindowAttribute(
            win32.hwnd.get(),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
                            toggle_overlay(&win);
                        }
                    }
                })
                .build(),
        )
        .manage(AppState::load())
        .setup(|app| {
            let window = app
                .get_webview_window(OVERLAY_LABEL)
                .expect("overlay window is defined in tauri.conf.json");

            apply_overlay_effects(&window);

            // Register the configured global hotkey.
            let hotkey = {
                let state = app.state::<AppState>();
                let cfg = state.config.lock().unwrap();
                cfg.hotkey.clone()
            };
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            if let Err(e) = app.global_shortcut().register(hotkey.as_str()) {
                eprintln!("failed to register hotkey `{hotkey}`: {e}");
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Overlay dismisses itself the moment it loses focus.
            WindowEvent::Focused(false) => {
                if window.label() == OVERLAY_LABEL {
                    let _ = window.hide();
                }
            }
            // Closing just hides — the app keeps running for the next hotkey press.
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == OVERLAY_LABEL {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_repos,
            commands::rescan_repos,
            commands::search_repos,
            commands::build_actions,
            commands::run_action,
            commands::hide_overlay,
        ])
        .run(tauri::generate_context!())
        .expect("error while running dev-prompt");
}
