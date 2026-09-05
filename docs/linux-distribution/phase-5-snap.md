# Phase 5 — Snap Store

**Lift:** M, but only **after Phase 4** (it reuses the sandbox plumbing).
**Reach:** Ubuntu and derivatives primarily; any distro with `snapd`.
**Needs from maintainer:** a Snapcraft (Ubuntu One) account.

## Why after Flatpak

Snap's confinement raises the same wall as Flatpak: a strictly-confined snap
can't exec arbitrary host binaries. The fix is the same shape as Phase 4's
`flatpak-spawn --host` — so do Flatpak first, then Snap is mostly manifest work
plus swapping the host-spawn mechanism.

## Confinement decision

| Mode | Host program launching | Store treatment |
|---|---|---|
| `strict` | not possible without heroics | preferred, auto-reviewed |
| `classic` | full host access, behaves like a native install | **manual review**, must justify why strict is impossible |

dev-prompt is a launcher for host tools → `classic` is the honest fit, the same
way IDEs and editors ship as classic snaps. Expect to write a justification for
the reviewers ("must run the user's own `code`, `nvim`, terminal, etc. on the
host; strict confinement defeats the purpose"). Budget review round-trips.

If you want `strict`: you'd need a `flatpak-spawn`-equivalent. Snap has no clean
one; `snapctl` doesn't do it. Options are `--host`-style helpers via a content
interface or asking users to install a companion — not worth it. Go `classic`.

## `snap/snapcraft.yaml`

- `base: core24`, `confinement: classic`, `grade: stable`.
- `adopt-info` from a part that runs `scripts/version.mjs` (or parse the git
  tag) so `version:` tracks CalVer.
- Parts: a `rust` + `npm` build of `src-tauri` (`npm ci` → `npm run tauri build
  --no-bundle`), stage `dev-prompt` + the Phase-1 `.desktop`/metainfo/icons.
- `apps.dev-prompt.command`, `apps.dev-prompt.desktop`.
- classic confinement means no `plugs:` needed, but the binary must be built
  against libraries present on the base or bundled/`patchelf`'d (Snap's classic
  runtime linker rpath dance — `craftctl`/`snapcraftctl` handles the common
  cases; verify webkit2gtk-4.1 resolves).
- Tray: classic snaps see the host's SNI host; ensure `libayatana-appindicator3`
  is staged or present.

## Updater

Snap auto-refreshes. Same as Flatpak: disable the in-app updater when the
`SNAP` env var is set (reuse the Phase-4 `is_sandboxed` gate — extend it to
check `SNAP` too).

## Release automation

- `snapcraft` build in CI, `snapcraft upload --release=stable` with a
  `SNAPCRAFT_STORE_CREDENTIALS` repo secret (exported via `snapcraft export-login`).
- Or connect the Snapcraft "Build" service to the GitHub repo and let it build
  on tag.

## Definition of done

- [ ] `snap install dev-prompt --classic` works on Ubuntu; tray + hotkey +
      launching host editors/terminals all work.
- [ ] Store listing has icon, screenshots (from Phase 1), description.
- [ ] Updater UI absent under Snap.
- [ ] CI uploads a new revision on every release.
- [ ] `docs/linux-distribution/README.md` status updated.
