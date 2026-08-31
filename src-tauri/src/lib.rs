mod cache;
mod commands;
mod config;
mod error;
mod index;
mod inspect;
mod launch;
mod rules;
mod scan;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

use commands::AppState;

const OVERLAY_LABEL: &str = "overlay";

/// Bring the overlay up: centered, focused, on top. `set_always_on_top` may have
/// been dropped by "open config.yaml", so re-assert it here.
fn show_overlay(window: &WebviewWindow) {
    let _ = window.center();
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("overlay:shown", ());
}

/// Show the overlay, or hide it if it is already visible.
fn toggle_overlay(window: &WebviewWindow) {
    match window.is_visible() {
        Ok(true) => {
            let _ = window.hide();
        }
        _ => show_overlay(window),
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

/// Arg the autostart entry is registered with — its presence means "launched at
/// login", so the overlay stays hidden until the hotkey. A normal user launch
/// (Start menu, double-click) omits it and we pop the overlay so people can see
/// the app is running.
const AUTOSTART_ARG: &str = "--autostart";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // Single-instance must be registered first: a second launch focuses the
    // running overlay instead of starting a duplicate (no second tray icon, no
    // failed hotkey re-registration).
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
            show_overlay(&w);
        }
    }));

    let mut builder = builder
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init());

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec![AUTOSTART_ARG]),
            ));
    }

    builder
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

            // System-tray entry point — the window is otherwise invisible with
            // no taskbar item, so the tray is the only way to reach settings or
            // quit the app.
            let show_i =
                MenuItem::with_id(app, "tray-show", "Show dev-prompt", true, None::<&str>)?;
            let settings_i =
                MenuItem::with_id(app, "tray-settings", "Settings…", true, None::<&str>)?;
            let quit_i =
                MenuItem::with_id(app, "tray-quit", "Quit dev-prompt", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show_i,
                    &settings_i,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_i,
                ],
            )?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("bundled window icon").clone())
                .tooltip("dev-prompt")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "tray-show" => {
                        if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
                            show_overlay(&w);
                        }
                    }
                    "tray-settings" => {
                        if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
                            show_overlay(&w);
                            let _ = w.emit("overlay:goto-settings", ());
                        }
                    }
                    "tray-quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(w) =
                            tray.app_handle().get_webview_window(OVERLAY_LABEL)
                        {
                            toggle_overlay(&w);
                        }
                    }
                })
                .build(app)?;

            // First run: opt into start-at-login (the user can turn it off in
            // Settings; we never re-enable).
            #[cfg(desktop)]
            {
                use tauri_plugin_autostart::ManagerExt;
                let first_run = app.state::<AppState>().first_run;
                if first_run {
                    let _ = app.autolaunch().enable();
                }
            }

            // Show the overlay on a normal launch so the app is visibly there;
            // stay hidden when started at login.
            if !std::env::args().any(|a| a == AUTOSTART_ARG) {
                show_overlay(&window);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Overlay dismisses itself on focus loss — unless the frontend has
            // opted out (settings screen).
            WindowEvent::Focused(false) => {
                if window.label() == OVERLAY_LABEL {
                    let dismiss = window
                        .app_handle()
                        .try_state::<AppState>()
                        .map(|s| *s.dismiss_on_blur.lock().unwrap())
                        .unwrap_or(true);
                    if dismiss {
                        let _ = window.hide();
                    }
                }
            }
            // Closing just hides — the app keeps running for the next hotkey press.
            WindowEvent::CloseRequested { api, .. } if window.label() == OVERLAY_LABEL => {
                api.prevent_close();
                let _ = window.hide();
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
            commands::get_config,
            commands::config_summary,
            commands::reload_config,
            commands::save_config,
            commands::open_rules_file,
            commands::set_dismiss_on_blur,
            commands::set_update_hint,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running dev-prompt");
}
