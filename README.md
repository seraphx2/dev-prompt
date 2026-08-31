# dev-prompt

A lightning-fast, cross-platform command-palette overlay for launching dev
repositories. Press a global hotkey, fuzzy-search your repos, hit Enter to open a
terminal / Claude Code / VS Code at that path.

Built with **Tauri v2** (Rust) + **Svelte 5** + **Tailwind CSS**.

> **Status: Milestone 1 (MVP).** Global-hotkey overlay, multi-root scan + cache,
> fuzzy search, and universal launch actions. The contextual inspection engine
> (`.sln` / `package.json` detection) and the YAML-driven rule system land in
> Milestones 2–3. Windows is the primary target; Linux/macOS paths are stubbed.

## Prerequisites

| Tool | Notes |
|------|-------|
| Node.js 18+ | frontend build (`npm`) — **installed** |
| Rust (stable) | `https://rustup.rs` — **required, not yet installed on this machine** |
| MSVC Build Tools | "Desktop development with C++" workload (Windows linker) |
| WebView2 runtime | preinstalled on Windows 11 |

Install Rust + MSVC, then everything below works.

## Run (development)

```sh
npm install
npm run tauri dev
```

`npm run tauri dev` starts Vite on :1420 and launches the overlay window
(hidden until you press the hotkey).

## Build

```sh
npm run tauri build
```

## Usage

- **Toggle overlay:** `Ctrl+Shift+Space` (configurable)
- **Move selection:** `Up` / `Down`
- **Launch default action:** `Enter`
- **Open action menu for a repo:** `Tab` / `Ctrl+Enter`
- **Force rescan:** `Ctrl+R`
- **Dismiss:** `Esc` or click away (overlay auto-hides on blur)

## Configuration

First run writes `config.yaml` to your OS config dir:

- Windows: `%APPDATA%\dev-prompt\config.yaml`
- Linux: `~/.config/dev-prompt/config.yaml`
- macOS: `~/Library/Application Support/dev-prompt/config.yaml`

```yaml
hotkey: CmdOrCtrl+Shift+Space
roots:
  - D:\git
  - ~/src
scan:
  max_depth: 4
  sentinels: [".git", ".sln", "package.json", "Cargo.toml", "pyproject.toml", "go.mod"]
cache_ttl_secs: 900
```

Edit `roots` to point at your code folders, then press `Ctrl+R` in the overlay.
The discovered repo list is cached at `<OS cache dir>/dev-prompt/repos.json` for
instant startup.

## Project layout

```
src/                     Svelte frontend (overlay UI)
  App.svelte             search box + results list + action menu
  lib/ipc.ts             typed wrappers over Tauri commands
  lib/components/        SearchInput, ResultList, ResultRow, ActionMenu
src-tauri/               Rust backend
  src/config.rs          load/save config.yaml, path expansion
  src/scan.rs            directory walk -> Vec<Repo>
  src/cache.rs           repos.json read/write, staleness, merge
  src/index.rs           nucleo fuzzy ranking
  src/actions.rs         per-repo action list (M1: universal only)
  src/launch.rs          detached process spawning
  src/commands.rs        #[tauri::command] surface + shared state
  src/lib.rs             plugin wiring, window setup, hotkey, blur, focus-loss hide
```

## Tests

```sh
cd src-tauri && cargo test
```

Covers config path expansion, sentinel detection / nested-repo collapsing, cache
staleness, and fuzzy ranking order.
