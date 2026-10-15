#!/usr/bin/env bash
#
# Lay out the website at https://seraphx2.github.io/dev-prompt/ under <site-dir>:
#
#   index.html          the product page (packaging/site/index.html, with the
#                       release version filled in for its download links)
#   img/                the screenshots + demo GIF from docs/img/, and the icon
#   install/index.html  the apt / dnf / pacman / Flatpak repository setup
#                       (packaging/repo/render-index.sh)
#
#   build-site.sh <site-dir> <version>      e.g. build-site.sh site 2026.1015.1
#
# Run from the repo root. The package repositories themselves (deb/, rpm/,
# arch/, flatpak/, dev-prompt.asc) live alongside in <site-dir>; build-repo.sh
# puts them there and calls this. To preview just the site:
#
#   bash packaging/site/build-site.sh /tmp/site 2026.1015.1
#   npx vite /tmp/site          (or any static server)
set -euo pipefail

SITE=${1:?site dir}
VERSION=${2:?version, e.g. 2026.1015.1}
VERSION=${VERSION#v}
repo_root=$(pwd)

mkdir -p "$SITE/img" "$SITE/install"

sed "s/@VERSION@/$VERSION/g" "$repo_root/packaging/site/index.html" > "$SITE/index.html"

cp "$repo_root"/docs/img/*.png "$repo_root"/docs/img/*.gif "$SITE/img/"
cp "$repo_root/src-tauri/icons/128x128.png" "$SITE/img/icon.png"

"$repo_root/packaging/repo/render-index.sh" > "$SITE/install/index.html"

echo "site laid out under $SITE (v$VERSION)"
