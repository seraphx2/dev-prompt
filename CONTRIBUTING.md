# Contributing to dev-prompt

Thanks for taking a look. This is a young, single-maintainer project — issues and
PRs are welcome, and the guidance below is meant to keep things quick rather than
formal. No CLA; the project is MIT.

## Dev setup

Prerequisites are in the [README](README.md#prerequisites). Then:

```sh
npm install
npm run tauri dev      # Vite on :1420 + the overlay (hidden until the hotkey)
```

CI runs on every PR (`.github/workflows/ci.yml`) and must be green before merge.
Run the same checks locally first:

```sh
npm run build                                    # svelte-check + vite build
cd src-tauri && cargo clippy --all-targets -- -D warnings
cd src-tauri && cargo test
```

(The Rust code is hand-formatted, not `rustfmt`-shaped — match the surrounding
style rather than running `cargo fmt`.)

## The easy, high-value path: teach it a new tool

Most support — a new editor, build tool, or language ecosystem — is **pure data
in [`src-tauri/src/default_config.yaml`](src-tauri/src/default_config.yaml)**, no
Rust required. That file has inline docs, and every field is spelled out in
[`docs/rules-engine.md`](docs/rules-engine.md); the shape is:

- **`markers`** — filenames/globs that make a directory show up as a repo.
- **`programs`** — how to find an executable, with per-OS candidate lists
  (`any` / `windows` / `linux` / `macos`): a bare name (PATH lookup), an absolute
  path, or a glob.
- **`rules`** — `match` a manifest glob, emit `actions`. A rule's `match` also
  counts as a discovery marker.
- **`universal`** — actions offered for every repo.

Gating keeps additions safe for people who don't have the tool:

- `requires: [mvn]` on a rule — skipped unless `mvn` is on `PATH`.
- `needs: [idea]` on an action — hidden unless the `idea` program resolves.

Nothing errors when a tool is absent; it just doesn't appear. If you add a rule,
add a matching assertion to the `bundled_defaults` tests in
[`src-tauri/src/config.rs`](src-tauri/src/config.rs).

## Larger changes

The roadmap — new providers, frontend modes, platform work — lives in
[`docs/config-design.md`](docs/config-design.md), roughly ordered easiest to
hardest. For anything non-trivial, open an issue referencing the relevant item
before you start, so we don't duplicate effort or design in opposite directions.

Rust lives in `src-tauri/src/` (see the layout in the README), the Svelte UI in
`src/`. Match the surrounding style — comment density, naming, and idiom included.

## Platforms

Windows is built, packaged, and tested. Linux and macOS share the same codebase
and compile; fixes for them are very welcome. The platform-specific seams are
small and localized:

- `#[cfg(windows)]` blocks in `src-tauri/src/lib.rs` (acrylic blur, rounded
  corners).
- `terminalize()` in `src-tauri/src/rules.rs` — Windows wraps commands in
  `wt.exe`; other platforms need per-emulator working-directory flags (see
  `docs/config-design.md`).
- Global hotkey on Wayland needs the XDG global-shortcuts portal; X11 and macOS
  work through the plugin as-is.

## Commits & PRs

- Keep commit subjects short and descriptive. Release versioning is date-based
  (CalVer), so there's no conventional-commits requirement.
- One logical change per PR where practical. Note any platform you couldn't test.
