// The app is Windows-first: a lot of `apps` / `rules` code (discovery, .lnk
// handling, arg splitting) sits behind `#[cfg(windows)]`, leaving its helpers,
// imports and `mut` bindings unreferenced off Windows. The non-Windows CI job
// exists for fast correctness feedback on the portable code, not as a style
// gate — the Windows build (local dev + release.yml's `tauri build`) keeps the
// full `-D warnings`. Tighten this once real cfg(unix) code exists on the other
// side of the split.
#![cfg_attr(not(windows), allow(dead_code, unused_imports, unused_mut))]

mod apps;
mod cache;
mod commands;
mod config;
mod dotnet;
mod error;
mod gowork;
mod gradle;
mod index;
mod inspect;
mod launch;
mod maven;
mod rules;
mod scan;
mod usage;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

use commands::AppState;

const OVERLAY_LABEL: &str = "overlay";

/// Bring the overlay up: centered, focused, on top. `set_always_on_top` may have
/// been dropped by "open config.yaml", so re-assert it here. `scope` is
/// `"repos"` normally, or `"apps"` when opened via the app-launcher hotkey — the
/// frontend seeds the search box accordingly.
fn show_overlay_scoped(window: &WebviewWindow, scope: &str) {
    let _ = window.center();
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("overlay:shown", scope);
}

fn show_overlay(window: &WebviewWindow) {
    show_overlay_scoped(window, "repos");
}

/// Tell the frontend the overlay is going away so it resets to the repo list
/// while off-screen. The next `show_overlay` then paints the home view directly
/// — without this, slow machines briefly show the previous screen (action menu,
/// settings) before Svelte catches up and snaps back. Works for both a
/// `WebviewWindow` (hotkey/tray) and a bare `Window` (window-event callback).
fn signal_overlay_hidden<R: tauri::Runtime>(window: &impl Emitter<R>) {
    let _ = window.emit("overlay:hidden", ());
}

/// Drop the first-run "stay open" latch. Called the first time the user
/// explicitly dismisses the overlay (hotkey toggle-off, `hide_overlay`, close),
/// after which normal dismiss-on-blur resumes. A no-op on every later launch.
fn release_sticky_open(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.sticky_open.lock().unwrap() = false;
    }
}

/// Show the overlay in `scope`, or hide it if it is already visible.
fn toggle_overlay_scoped(window: &WebviewWindow, scope: &str) {
    match window.is_visible() {
        Ok(true) => {
            release_sticky_open(window.app_handle());
            signal_overlay_hidden(window);
            let _ = window.hide();
        }
        _ => show_overlay_scoped(window, scope),
    }
}

fn toggle_overlay(window: &WebviewWindow) {
    toggle_overlay_scoped(window, "repos");
}

/// True when `accel` parses to the same shortcut the OS just reported.
fn shortcut_is(accel: &str, fired: &tauri_plugin_global_shortcut::Shortcut) -> bool {
    accel
        .parse::<tauri_plugin_global_shortcut::Shortcut>()
        .map(|s| &s == fired)
        .unwrap_or(false)
}

/// True when two accelerator strings resolve to the same shortcut, so
/// `Shift+CmdOrCtrl+X` and `CmdOrCtrl+Shift+X` count as unchanged. Unparseable
/// input matches nothing — callers validate parseability separately.
pub(crate) fn same_shortcut(a: &str, b: &str) -> bool {
    use tauri_plugin_global_shortcut::Shortcut;
    matches!(
        (a.parse::<Shortcut>(), b.parse::<Shortcut>()),
        (Ok(x), Ok(y)) if x == y
    )
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

/// Arg the autostart entry is registered with, so a login launch stays
/// distinguishable from a manual one in the process args. Nothing branches on it
/// today — first run is the only launch that shows the overlay; every other
/// launch (manual or at login) starts silent in the tray — but it's kept so
/// existing autostart Run keys don't need rewriting and a future "started at
/// login" check has something to read.
const AUTOSTART_ARG: &str = "--autostart";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // Single-instance must be registered first: it holds an OS lock so a second
    // `dev-prompt.exe` hands off to the running instance and exits before it can
    // become a window, tray icon, or a second hotkey registration. The re-launch
    // is deliberately inert — no window pops; the hotkey or tray is how you
    // reach a running instance.
    #[cfg(desktop)]
    let builder =
        builder.plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}));

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
                .with_handler(|app, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let Some(win) = app.get_webview_window(OVERLAY_LABEL) else {
                        return;
                    };
                    // The app-launcher hotkey opens straight into the `>` scope.
                    let scope = {
                        let state = app.state::<AppState>();
                        let cfg = state.config.lock().unwrap();
                        match cfg.apps_hotkey.as_deref() {
                            Some(h) if !h.is_empty() && shortcut_is(h, shortcut) => "apps",
                            _ => "repos",
                        }
                    };
                    toggle_overlay_scoped(&win, scope);
                })
                .build(),
        )
        .manage(AppState::load())
        .setup(|app| {
            let window = app
                .get_webview_window(OVERLAY_LABEL)
                .expect("overlay window is defined in tauri.conf.json");

            apply_overlay_effects(&window);

            // Register the configured global hotkey(s).
            let (hotkey, apps_hotkey) = {
                let state = app.state::<AppState>();
                let cfg = state.config.lock().unwrap();
                (cfg.hotkey.clone(), cfg.apps_hotkey.clone())
            };
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            if let Err(e) = app.global_shortcut().register(hotkey.as_str()) {
                eprintln!("failed to register hotkey `{hotkey}`: {e}");
            }
            if let Some(ah) = apps_hotkey.as_deref().filter(|h| !h.is_empty()) {
                if let Err(e) = app.global_shortcut().register(ah) {
                    eprintln!("failed to register apps hotkey `{ah}`: {e}");
                }
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

            // First run only: pop the overlay and hold it open (the `sticky_open`
            // latch makes focus loss non-dismissing until the user acts) so it's
            // unmistakable that the install finished and the app is live. Every
            // other launch, manual or at login, starts silent in the tray and
            // waits for the hotkey.
            //
            // Start-at-login is set by the installer (NSIS_HOOK_POSTINSTALL), not
            // here — keying it off `first_run` was unreliable, since a leftover
            // `config.yaml` (dev builds, or a prior install the uninstaller left
            // behind) makes `first_run` false on a genuine first run.
            if app.state::<AppState>().first_run {
                show_overlay(&window);
            }

            // Warm the process-global program-resolution cache off the UI thread
            // so the first action menu / launch doesn't pay for globbing, PATH
            // scans and vswhere on the main thread the way it used to.
            //
            // If a config reload (`clear_program_cache`) lands while this is
            // still resolving, a stale entry can survive until the next reload.
            // The window is ~1s at startup and self-heals, so it's not worth a
            // cache generation counter to close.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let cfg = handle.state::<AppState>().config.lock().unwrap().clone();
                    let resolver = crate::rules::Resolver::new(&cfg.programs)
                        .with_terminal(
                            cfg.terminal.as_deref(),
                            cfg.terminal_template.as_deref(),
                        )
                        .with_shell(cfg.shell.as_deref());
                    for key in cfg.programs.keys() {
                        let _ = resolver.resolve(key);
                    }
                    let _ = resolver.resolve("terminal");
                });
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
                        .map(|s| {
                            *s.dismiss_on_blur.lock().unwrap()
                                && !*s.sticky_open.lock().unwrap()
                        })
                        .unwrap_or(true);
                    if dismiss {
                        signal_overlay_hidden(window);
                        let _ = window.hide();
                    }
                }
            }
            // Closing just hides — the app keeps running for the next hotkey press.
            WindowEvent::CloseRequested { api, .. } if window.label() == OVERLAY_LABEL => {
                api.prevent_close();
                release_sticky_open(window.app_handle());
                signal_overlay_hidden(window);
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_repos,
            commands::rescan_repos,
            commands::search_repos,
            commands::build_actions,
            commands::repo_rule_trace,
            commands::refresh_repo_context,
            commands::run_action,
            commands::hide_overlay,
            commands::get_config,
            commands::config_summary,
            commands::reload_config,
            commands::save_config,
            commands::list_terminals,
            commands::list_shells,
            commands::run_command,
            commands::list_apps,
            commands::rescan_apps,
            commands::run_app,
            commands::open_rules_file,
            commands::open_releases_page,
            commands::set_dismiss_on_blur,
            commands::set_update_hint,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running dev-prompt");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn shortcut_is_ignores_spelling_and_modifier_order() {
        let fired: Shortcut = "CmdOrCtrl+Shift+Period".parse().unwrap();
        assert!(shortcut_is("CmdOrCtrl+Shift+Period", &fired));
        assert!(shortcut_is("shift+ctrl+Period", &fired)); // aliases + order
        assert!(!shortcut_is("CmdOrCtrl+Shift+Space", &fired));
        assert!(!shortcut_is("gibberish", &fired)); // unparseable -> no match
    }

    #[test]
    fn same_shortcut_compares_by_meaning_not_text() {
        assert!(same_shortcut("CmdOrCtrl+Shift+Period", "shift+ctrl+Period"));
        assert!(!same_shortcut("CmdOrCtrl+Shift+Period", "CmdOrCtrl+Shift+Space"));
        assert!(!same_shortcut("gibberish", "gibberish")); // unparseable != unparseable
    }

    #[test]
    fn both_bundled_hotkeys_parse() {
        let cfg = config::bundled_defaults();
        cfg.hotkey.parse::<Shortcut>().expect("repo hotkey");
        cfg.apps_hotkey
            .as_deref()
            .expect("apps_hotkey ships on by default")
            .parse::<Shortcut>()
            .expect("apps hotkey");
    }
}
