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
- macOS — `~/Library/Application Support/dev-prompt/`

**This document covers settings (`config.yaml`).** For the rule engine — markers,
programs, rules, actions, universal, template variables — see
**[rules-engine.md](rules-engine.md)**.

Everything below is set from the **Settings** screen and stored in `config.yaml`.
Changes apply the moment you hit **Save** (the hotkey re-registers live);
`roots` / `scan` / `cache_ttl_secs` take effect on the next rescan.

---

## Hotkeys

Two global hotkeys, set from **Settings ▸ Global hotkeys**:

```yaml
# config.yaml
hotkey: CmdOrCtrl+Shift+Space        # opens the overlay on the repo browser
apps_hotkey: CmdOrCtrl+Shift+Period  # opens straight into the ">" app launcher
                                     #   ("" = off; the Settings toggle writes it)
```

`apps_hotkey` is a convenience — it's the same as pressing the main hotkey then
typing `>`. It's on by default (`Ctrl+Shift+.`, i.e. `Ctrl+>`); the **App
launcher** field has a *turn off* link. The two must be different.

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

## Terminal

Which terminal emulator "Open in terminal" and every `terminal: true` action
open. Set from **Settings ▸ Terminal**. **Windows only** for now — other
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

### Shell

A one-shot command is wrapped in a shell so the window stays open and keeps a
real console (ANSI colour, a live TTY — tools like Claude Code need it). The
shell is `pwsh` (else Windows PowerShell) unless you set **Settings ▸ Shell** /
`shell:` — `cmd`, `bash`, `nu`, … are recognised for their "run and hold" flags.
The **Run command…** action picks a shell per-run, defaulting to that setting.

---

## Apps

Type `>` in the search bar to switch the list from repositories to **installed
applications** — or press the [app-launcher hotkey](#hotkeys) to open there
directly. Delete back to an empty box (or clear it) to return — the repo list is
always the default when the overlay opens the normal way. Enter launches the
selected app; `Ctrl+R` re-enumerates. Frecency: apps you launch from dev-prompt
float to the top of the empty-query list.

**Windows only.** On other platforms the `>` scope shows an empty list.

Discovery unions four sources and de-duplicates by executable path:

- **Start Menu** shortcuts (both the machine and per-user `Programs` trees)
- **Store apps** (`Get-StartApps` AppUserModelIDs)
- the three **Uninstall** registry hives (`HKLM`, `HKLM\WOW6432Node`, `HKCU`)
- a bounded `*.exe` scan of `%LOCALAPPDATA%\Programs` plus any `extra_dirs`

Icons are extracted from the executables and cached under
`%LOCALAPPDATA%\dev-prompt\cache\app-icons\`.

```yaml
# config.yaml — managed from Settings ▸ "Index installed apps"
apps:
  enabled: true
  extra_dirs:                 # extra folders to scan for portable executables
    - D:\tools
  exclude:                    # drop apps whose name or path contains any of
    - zoom                    #   these (case-insensitive)
```

Installer stubs, updaters, redistributables, crash handlers and OS components
under `\Windows\` are filtered out automatically; `exclude` is for the rest.

---

## Roots, scan depth, cache

```yaml
# config.yaml
roots:                        # folders to scan for repos — Settings ▸ Roots
  - D:\git
scan:
  max_depth: 4                # how deep under a root to look
  collapse_nested: true       # true = hide a repo found inside another repo;
                              #   false = list every one; auto = drop it unless
                              #   it looks independent (real .git dir, not in the
                              #   ancestor's .gitmodules, not under vendor/ …)
cache_ttl_secs: 900           # how long the discovered repo list stays fresh
                              #   before the next open triggers a background rescan
```

The discovered list is cached at `<OS cache dir>/dev-prompt/repos.json`.

---

## Applying changes

- Settings changes apply the moment you hit **Save** — the hotkey re-registers
  live; `roots` / `scan` / `cache_ttl_secs` take effect on the next rescan
  (`Ctrl+R` forces one).
- To reload `rules.yaml`, use **Settings ▸ Rules ▸ Reload config** — see
  [rules-engine.md](rules-engine.md#applying-changes).
