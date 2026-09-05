# docs/img/

Public image assets referenced by docs and packaging metadata.

## `overlay.png` — REQUIRED before Phase 4 (Flathub)

`packaging/linux/io.github.seraphx2.devprompt.metainfo.xml` references
`overlay.png` here as the AppStream screenshot. Flathub requires at least one
screenshot; it must be a PNG or JPEG, no wider than 2000px, ideally showing the
overlay in normal use (hotkey pressed, a repo search in progress). Capture it on
a clean desktop and commit it as `docs/img/overlay.png`.

Until then the metainfo points at a URL that 404s — fine for
`appstreamcli validate`, but the Flathub build will fail.
