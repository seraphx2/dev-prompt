//! Flatpak login-autostart via the XDG Background portal.
//!
//! The native builds use `tauri-plugin-autostart`, which writes
//! `~/.config/autostart/dev-prompt.desktop` directly. Inside the Flatpak sandbox
//! that path isn't writable, so we ask `xdg-desktop-portal` to register the
//! entry on our behalf (`org.freedesktop.portal.Background.RequestBackground`).
//! Reached through the always-available portal proxy — no `finish-args` needed.

use ashpd::desktop::background::Background;

/// Ask the portal to add (or remove) a login autostart entry for this app. The
/// first enable pops a system consent dialog; subsequent calls are silent.
/// Returns the autostart state the portal actually granted (a denied dialog
/// comes back `false`, not an error).
pub async fn portal_set(enabled: bool) -> ashpd::Result<bool> {
    let response = Background::request()
        .reason("Start dev-prompt at login so its global hotkey is always available.")
        .auto_start(enabled)
        // Match the native autostart entry; a non-first-run launch starts silent
        // in the tray regardless, so this is mostly for parity / future use.
        .command(["dev-prompt", "--autostart"])
        .dbus_activatable(false)
        .send()
        .await?
        .response()?;
    Ok(response.auto_start())
}

/// Best-effort read of the current state. The portal writes the autostart file
/// to the host's `$HOME/.config/autostart/<app-id>.desktop`, which is visible to
/// us because the manifest grants `--filesystem=home` (so `$HOME` is the real
/// home). If the user later removes it via their desktop this can read stale,
/// which is a cosmetic checkbox issue only.
pub fn is_enabled() -> bool {
    let Ok(home) = std::env::var("HOME") else {
        return false;
    };
    std::path::Path::new(&home)
        .join(".config/autostart/io.github.seraphx2.devprompt.desktop")
        .is_file()
}
