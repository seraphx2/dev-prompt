# Configuration

dev-prompt has three config layers:

| | |
|---|---|
| **`default_config.yaml`** | Bundled into the binary — the baseline for *everything*: settings defaults plus the full set of markers, programs, rules, and universal actions. You never edit it. Canonical copy: [`src-tauri/src/default_config.yaml`](../src-tauri/src/default_config.yaml). |
| **`config.yaml`** | Your **settings** — hotkeys, roots, scan depth, cache lifetime, terminal, shell, apps. Managed entirely by the Settings screen; you don't normally open it by hand. |
| **`rules.yaml`** | Your **rule-engine overrides** — extra markers, programs, rules, and universal actions layered over the defaults. Hand-authored. The Settings screen never rewrites it, so your comments stay put. |

Both user files live in your OS config directory, created on first run:

- Windows — `%APPDATA%\dev-prompt\`
- Linux — `~/.config/dev-prompt/`

**This document covers settings (`config.yaml`).** For the rule engine — markers,
programs, rules, actions, universal, template variables — see
**[rules-engine.md](rules-engine.md)**.

Everything below is set from the **Settings** screen and stored in `config.yaml`.
There's no Save button: each change saves itself (a typed value after a short
pause, a root folder when you leave its field) and the hotkey re-registers
live. Only valid values are saved — a newly typed folder has to exist, a
custom template needs its placeholders, a number has to be in range. An
invalid one is marked in red and left unsaved; close Settings with one still
there and the footer says what wasn't saved.

---

## Hotkeys

Two global hotkeys, set from **Settings ▸ General ▸ Global hotkeys**:

```yaml
# config.yaml
hotkey: CmdOrCtrl+Backslash    # opens the overlay on the repo browser
apps_hotkey: CmdOrCtrl+Period  # opens straight into the ">" app launcher
                               #   ("" = off; the Settings toggle writes it)
```

`apps_hotkey` is a convenience — it's the same as pressing the main hotkey then
typing `>`. It's on by default (`Ctrl+.`); the **App launcher** field has a
*turn off* link. The two must be different.

The recorder (click a field, press a combination) checks what you pick against a
built-in list: it **refuses** combos the OS reserves or that can't be
intercepted (`Alt+Tab`, `Win+L`, `Ctrl+Alt+Del`, bare keys, …) and **asks you
to confirm** ones commonly used elsewhere (`Ctrl+Shift+N`, a lone
`Ctrl+`*letter*, any `Win+`*key*, …). This is a maintained list, not detection —
Windows has no API for the shortcuts individual apps use internally, so a combo
can still collide with something the list doesn't know. If another program has
already claimed a combo through the OS, registration fails and the old hotkey
stays active.

---

## Start at login

**Settings ▸ General ▸ Start at login** registers dev-prompt to launch (silently, into the
tray) when you log in — recommended, since the app is only useful when it's
already running to catch the hotkey. The OS is the source of truth: the checkbox reflects the actual autostart entry.

- **Windows / native Linux** — writes the platform autostart entry directly
  (`~/.config/autostart/dev-prompt.desktop` or a `Run` key).
- **Flatpak** — the sandbox can't write that file, so the toggle asks
  `xdg-desktop-portal` to do it. The **first** enable shows a system consent
  dialog ("Allow dev-prompt to start automatically?"); after that it's silent.
- **AppImage** — no autostart hook. Add one yourself via your desktop's session
  / autostart settings, pointing at the AppImage with `--autostart`.

---

## Zoom

The overlay follows your display scaling (Windows' Scale setting, your Linux
desktop's) on its own. **Settings ▸ General ▸ Zoom** makes it bigger or smaller
on top of that — text, icons and the window together, so nothing gets cropped.
From anywhere in the overlay, the built-in shell included, <kbd>Ctrl</kbd>
<kbd>+</kbd> and <kbd>Ctrl</kbd> <kbd>−</kbd> step through 80 % to 200 %, and
<kbd>Ctrl</kbd> <kbd>0</kbd> goes back to 100 %. It's saved as:

```yaml
zoom: 1.25        # 125 %; left out at 100 %
```

---

## Terminal

Which terminal emulator "Open in terminal" and every `terminal: true` action
open. Set from **Settings ▸ Tools ▸ Terminal**. **Windows only** for now — other
platforms run the command directly.

```yaml
# config.yaml
terminal: wezterm                         # a programs.terminal key, a PATH
                                          #   name, or an absolute path.
                                          #   Absent = first one that resolves.
terminal_template: >                      # only for a terminal not in the
  wezterm start --cwd {{dir}} -- {{cmd}}   #   table below. {{dir}} = cwd,
                                          #   {{cmd}} = the command.
```

dev-prompt knows how to drive **Windows Terminal** (`wt`), **Alacritty**, and
**WezTerm** — pick any that's installed from the dropdown. For anything else,
choose *Custom…* and give a `terminal_template`: it's run verbatim with `{{dir}}`
and `{{cmd}}` substituted (put `{{cmd}}` after `--` or in quotes so its arguments
stay together).

Leaving `terminal:` unset (**Auto**) tries `wt.exe` first — except if you've
explicitly set Windows' own **Settings ▸ For developers ▸ Terminal** to the
classic **Windows Console Host**, in which case dev-prompt honors that and
skips straight to the next candidate (or, if nothing else resolves, runs the
shell directly instead of forcing Windows Terminal open against your choice).
Picking Windows Terminal there, or leaving it on "Let Windows decide", changes
nothing — both already agree with dev-prompt's own default.

### Shell

**Settings ▸ Tools ▸ Shell** / `shell:` picks the shell **Open shell** runs
inside the overlay. Unset, it's `pwsh` (else Windows PowerShell) on Windows and
your login shell (`$SHELL`) on Linux. The shell's name in its header
switches a single session to another one.

On Windows, `git-bash` is Git for Windows' bash and `wsl` your default WSL
distro. They're listed apart because the `bash` on a Windows PATH is usually
WSL's launcher; plain `bash` is offered only for another bash on PATH (MSYS2,
Cygwin, …). A `shell: bash` set before keeps meaning what it did.

The same shell wraps a one-shot command in an external terminal, so the window
stays open and keeps a real console (ANSI colour, a live TTY — tools like
Claude Code need it); `cmd`, `bash`, `nu`, … are recognised for their "run and
hold" flags.

`shell_font` / `shell_font_size` set its font. The family you give is
tried first, ahead of the default list (Nerd Font builds of Cascadia, then
Cascadia, Consolas, DejaVu Sans Mono); the size defaults to 13. Use a
[Nerd Font](https://www.nerdfonts.com/) if your prompt draws icons (oh-my-posh,
Starship).

```yaml
# config.yaml — managed from Settings ▸ Tools
shell: pwsh
shell_font: "JetBrainsMono Nerd Font Mono"
shell_font_size: 14
```

### File manager

Which program "Show in file manager" opens. Set from **Settings ▸ Tools ▸ File
manager**. Unlike Terminal, dev-prompt doesn't need a table of known
invocations here — nearly every file manager opens a folder given as a bare
path argument, the same contract `explorer.exe` has, so every installed
candidate found via `programs.filemanager` shows up in the dropdown.

```yaml
# config.yaml
filemanager: dopus                        # a programs.filemanager key, a PATH
                                          #   name, or an absolute path.
                                          #   Absent = first one that resolves.
filemanager_template: dopus /cmd Go {{path}}   # only for a file manager that
                                          #   wants more than a bare path.
```

If the one you pick needs more than a bare path (Directory Opus's `/cmd Go`,
say), choose *Custom…* and give a `filemanager_template`: it's expanded and
run verbatim, with `{{path}}` substituted for the folder.

---

## Apps

Type `>` in the search bar to switch the list from repositories to **installed
applications** — or press the [app-launcher hotkey](#hotkeys) to open there
directly. Delete back to an empty box (or clear it) to return — the repo list is
always the default when the overlay opens the normal way. Enter launches the
selected app; `Ctrl+R` re-enumerates. Frecency: apps you launch from dev-prompt
float to the top of the empty-query list.

Supported on **Windows** and **Linux**.

**Windows** discovery unions four sources and de-duplicates by executable path:

- **Start Menu** shortcuts (both the machine and per-user `Programs` trees)
- **Store apps** (`Get-StartApps` AppUserModelIDs)
- the three **Uninstall** registry hives (`HKLM`, `HKLM\WOW6432Node`, `HKCU`)
- a bounded `*.exe` scan of `%LOCALAPPDATA%\Programs` plus any `extra_dirs`

Icons are extracted from the executables and cached under
`%LOCALAPPDATA%\dev-prompt\cache\app-icons\`.

**Linux** parses freedesktop `.desktop` entries from `$XDG_DATA_HOME/applications`,
every `$XDG_DATA_DIRS/*/applications`, the Flatpak and Snap export directories,
and any `extra_dirs`. Entries with `NoDisplay=true`, `Hidden=true`, a missing
`TryExec`, or a non-`Application` type are skipped; a user override in
`~/.local/share` shadows the system copy of the same id. Icons are resolved
against the current icon theme (then Adwaita / breeze / Papirus / hicolor, then
`pixmaps`) and embedded in `apps.json`. Launch goes through `gtk-launch`, so
`Exec` field codes, `Terminal=true` and D-Bus activation are handled by the
platform.

```yaml
# config.yaml — managed from Settings ▸ Apps
apps:
  enabled: true
  extra_dirs:                 # extra folders to scan for portable executables
    - D:\tools
  exclude:                    # drop apps whose name or path contains any of
    - zoom                    #   these (case-insensitive)
```

Installer stubs, updaters, redistributables, crash handlers and OS components
under `\Windows\` are filtered out automatically; `exclude` is for the rest.

The discovered app list is cached at `<OS cache dir>/dev-prompt/apps.json`.

**Keeping it current.** While dev-prompt runs, it watches the places
installers leave their traces: on Windows the Start Menu folders,
`%LOCALAPPDATA%\Programs`, your `extra_dirs`, the Uninstall registry keys and
the Store package registry key; on Linux the application folders above. A few
seconds after an install or uninstall settles, only what changed is gathered
again — one shortcut, one app folder, the Uninstall entries whose timestamps
moved, or the Store list — and merged into the cached list, so a new app shows
up without a full rescan. The cache keeps every gathered row with where it came
from so this works; a cache written by an older version gets one full rescan
the first time something changes. A full rescan still runs once a day, and
`Ctrl+R` in `>` forces one.

---

## Roots, scan depth, cache

```yaml
# config.yaml
roots:                        # folders to scan for repos — Settings ▸ Repos
  - D:\git
scan:
  max_depth: 4                # how deep under a root to look
  collapse_nested: true       # true = hide a repo found inside another repo;
                              #   false = list every one; auto = drop it unless
                              #   it looks independent (real .git dir, not in the
                              #   ancestor's .gitmodules, not under vendor/ …)
  top_level_folders: true     # list every folder directly inside a root, even
                              #   with no marker (notes, documents, a project not
                              #   under version control yet) — except one that
                              #   only holds other repos. Deeper folders still
                              #   need a marker.
cache_ttl_secs: 900           # how long the discovered repo list stays fresh
                              #   before the next open triggers a background rescan
dismiss: always               # when the overlay closes itself — Settings ▸
                              #   General ▸ Overlay dismissal:
                              #   always       = on focus loss, and after an action
                              #   keep_on_blur = ignore focus loss; still close
                              #                  after an action or on Esc
                              #   manual       = only Esc / the hotkey / the tray
```

The discovered list is cached at `<OS cache dir>/dev-prompt/repos.json`.

While dev-prompt runs, it **watches your roots** and rescans about a second
after something changes the list: a repo created, deleted, renamed or moved
in, a marker file added or removed, or a folder added directly in a root. Work
inside a repo (commits, builds, editing files) doesn't trigger anything.
`cache_ttl_secs` is the backstop for changes a watcher can't see — network
shares that don't report changes, or on Linux, roots with more folders than the
system's inotify watch limit allows.

With `dismiss: manual`, launching an action from the menu leaves the overlay open
on that repo (filter cleared) so you can fire several in a row; `Esc` steps back
out as usual.

`keep_on_blur` and `manual` also give the overlay a **taskbar button** (so you
can click back to it after it loses focus); `always` keeps it out of the
taskbar. On Wayland this is a no-op — the overlay is always in the taskbar there
regardless.

---

## Applying changes

- Settings save as you change them — the hotkey re-registers live; a change to
  `roots` or `scan` rescans the repos (and the folder watcher follows the new
  roots) straight away, a change to `apps` re-gathers the app list, and
  `cache_ttl_secs` applies from the next time the cache is checked.
- To reload `rules.yaml`, use **Settings ▸ Active rules ▸ Reload rules** — see
  [rules-engine.md](rules-engine.md#applying-changes).
