# docs/img/

Public image assets referenced by docs and packaging metadata.

## Screenshots

`packaging/linux/io.github.seraphx2.devprompt.metainfo.xml` references these as
the AppStream `<screenshots>` (pulled by `raw.githubusercontent.com/.../main/...`
— they resolve once `dev` merges to `main`):

| File | Shows |
| --- | --- |
| `1_dev-prompt_repos.png` | repo list — fuzzy search over the discovered repos |
| `2_dev-prompt_actions.png` | a repo's action menu (detected scripts / tools) |
| `3_dev-prompt_apps.png` | the `>` installed-application launcher |

Captured on Linux (KDE, Wayland) at the overlay's native 720×480, against
`scripts/clone-showcase.mjs`'s demo repo set. Flathub wants PNG/JPEG, ≤2000px
wide. To refresh: run `node scripts/clone-showcase.mjs`, point a scan root at
that folder, set `dismiss: manual` so the overlay stays put, and grab the window
with Spectacle's rectangular-region + window-snap.
