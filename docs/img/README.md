# docs/img/

Public image assets referenced by docs and packaging metadata.

Everything here is generated, not captured by hand — after a UI change, one
command redoes the lot:

```sh
npm run screenshots                 # the demo GIF and every still (stills at 2x)
npm run screenshots -- --scale 1    # stills at the overlay's native pixel size
npm run screenshots -- --only 0     # just the GIF; or e.g. --only 4,5 for stills
```

## Demo

`0_dev-prompt_demo.gif`, at the top of the root `README.md`: search, the
action menu, Open shell (the overlay grows and the output streams in), back
out with the shell left running, the `$` list, and straight back into it —
about 14 s, looping. 1000×640: the shell-sized canvas, with the smaller screens
centred on it, so the panel grows from the middle as it does on screen. The
storyboard is `demo()` in `capture.mjs`; `gif.mjs` turns the frames into the
GIF (one shared palette, identical frames merged).

`0_dev-prompt_demo.mp4` is the same demo as a video (played twice), for places
that take video but not GIFs. It's made from the GIF with ffmpeg when that's
installed, and skipped otherwise.

## Screenshots

`scripts/screenshots/capture.mjs` runs the real frontend in headless Chromium
(Playwright) against a fake backend — `shots.ts` answers every Tauri call from
`demo-data.ts`: the `scripts/clone-showcase.mjs` repo set under
`/home/dev/code`, the actions the real backend produces for them, a few
installed apps, and recorded shell output. Each scene presses the keys a person
would, then captures `<main>` at exactly the overlay's size. So every image
shows the same data, no personal paths, and no desktop behind the panel.

| File | Shows | Size (2x) |
| --- | --- | --- |
| `1_dev-prompt_repos.png` | repo list, a shell left running in `hugo` | 1520×960 |
| `2_dev-prompt_actions.png` | `prettier`'s action menu with the quick-action row | 1520×960 |
| `3_dev-prompt_apps.png` | the `>` installed-application launcher | 1520×960 |
| `4_dev-prompt_shell.png` | Open shell in `hugo`: `git log`, then `hugo server` | 2000×1280 |
| `5_dev-prompt_shells.png` | the `$` running-shells list | 1520×960 |
| `6_dev-prompt_new-repo.png` | the New repository form | 1520×960 |
| `7_dev-prompt_remove.png` | Move to Trash, listing what would be lost | 1520×960 |

The README shows them at their on-screen size (`width="760"`, or `"1000"` for
the shell). `packaging/linux/io.github.seraphx2.devprompt.metainfo.xml` lists
them all as AppStream `<screenshots>`, pulled by
`raw.githubusercontent.com/.../main/...` — they resolve once `dev` merges to
`main`. Flathub wants PNG/JPEG, ≤2000px wide; the shell shot is exactly 2000 at
2x.

Fonts come from the machine that runs it (the app uses the system UI font and a
monospace stack), so for AppStream run it on Linux. The app is told it's on
Linux either way, so the wording ("Trash") and panel fill match.

To change what a shot shows, edit its scene in `capture.mjs` or the data in
`demo-data.ts`. A new screen the fake backend doesn't know yet usually needs a
case in `shots.ts`'s `backend()` for whatever it `invoke`s.
