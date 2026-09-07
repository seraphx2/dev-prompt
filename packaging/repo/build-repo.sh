#!/usr/bin/env bash
#
# Rebuild the apt + rpm + pacman repository trees under <site> from every package
# currently in the pools plus any new ones in <artifacts>, then sign all metadata
# with the repo GPG key. Also drops in the landing page, the signing key, and the
# Flatpak remote descriptors (the OSTree repo under <site>/flatpak is placed by
# repo.yml, not here).
#
#   build-repo.sh <artifacts-dir> <site-dir> <gpg-key-id>
#
# Run from the repo root (needs packaging/repo/dev-prompt-repo.asc and
# packaging/flatpak/*.flatpakre{f,po}). <site-dir> is a checkout / worktree of
# the gh-pages branch.
#
# Tools: apt-ftparchive (apt-utils), createrepo_c, gpg, rpm/rpmsign (rpm-sign on
# Fedora, rpm on Debian/Ubuntu), and repo-add — the last from `pacman`; if it
# isn't on PATH the script runs it in a throwaway `archlinux` container via
# docker (so this works on a Debian CI runner and on an Arch box unchanged).
set -euo pipefail

ARTIFACTS=${1:?artifacts dir}
SITE=${2:?site dir}
KEYID=${3:-E3C07CD21A9A9BA5}   # repo signing key; overridable for local tests
KEEP=${REPO_KEEP:-10}

repo_root=$(pwd)
gpg_bin=(gpg --batch --yes --pinentry-mode loopback --default-key "$KEYID")

mkdir -p \
  "$SITE/deb/pool/main/d/dev-prompt" \
  "$SITE/deb/dists/stable/main/binary-amd64" \
  "$SITE/rpm" \
  "$SITE/arch"

# ---------------------------------------------------------------- ingest + prune
shopt -s nullglob
for f in "$ARTIFACTS"/*.deb;         do cp -f "$f" "$SITE/deb/pool/main/d/dev-prompt/"; done
for f in "$ARTIFACTS"/*.rpm;         do cp -f "$f" "$SITE/rpm/"; done
for f in "$ARTIFACTS"/*.pkg.tar.zst; do cp -f "$f" "$SITE/arch/"; done
shopt -u nullglob

prune() { # <dir> <name-glob> — keep the newest $KEEP, delete the rest (+ .sig)
  local d=$1 pat=$2 all doomed f
  [ -d "$d" ] || return 0
  mapfile -t all < <(cd "$d" && find . -maxdepth 1 -name "$pat" -printf '%f\n' | sort -V)
  (( ${#all[@]} > KEEP )) || return 0
  doomed=( "${all[@]:0:${#all[@]}-KEEP}" )
  for f in "${doomed[@]}"; do
    echo "prune: $d/$f"
    rm -f -- "$d/$f" "$d/$f.sig"
  done
}
prune "$SITE/deb/pool/main/d/dev-prompt" '*.deb'
prune "$SITE/rpm" '*.rpm'
prune "$SITE/arch" '*.pkg.tar.zst'
# drop pacman sigs orphaned by a prune
for s in "$SITE"/arch/*.sig; do [ -e "$s" ] && [ ! -e "${s%.sig}" ] && rm -f "$s"; done
true

# ------------------------------------------------------------------------- apt
(
  cd "$SITE/deb"
  apt-ftparchive packages pool > dists/stable/main/binary-amd64/Packages
  gzip -9cf dists/stable/main/binary-amd64/Packages \
    > dists/stable/main/binary-amd64/Packages.gz
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=dev-prompt \
    -o APT::FTPArchive::Release::Label=dev-prompt \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    release dists/stable > dists/stable/Release
  "${gpg_bin[@]}" -abs      -o dists/stable/Release.gpg dists/stable/Release
  "${gpg_bin[@]}" --clearsign -o dists/stable/InRelease  dists/stable/Release
)

# ------------------------------------------------------------------------- rpm
# Sign every package. dnf's `gpgcheck=1` (dnf4 and dnf5) verifies the *package*
# signature, not just the repo metadata — an unsigned .rpm is refused at install
# with "The package is not signed." Signing is embedded in the rpm header, so it
# must happen before createrepo_c reads the checksums. Re-signing an already
# signed package just replaces the header sig, so signing all of them every run
# is fine (KEEP caps this at a handful of ~4 MB files).
# Override %__gpg_sign_cmd with an absolute gpg path (rpm doesn't PATH-search it)
# and loopback/batch flags so it never tries to prompt on a passphrase-less key.
rpm_sign_cmd="$(command -v gpg) --no-verbose --no-armor --batch --pinentry-mode loopback -u \"%{_gpg_name}\" -sbo %{__signature_filename} --digest-algo sha256 %{__plaintext_filename}"
for f in "$SITE"/rpm/*.rpm; do
  rpm --define "_gpg_name $KEYID" \
      --define "__gpg_sign_cmd $rpm_sign_cmd" \
      --addsign "$f"
done
rm -rf "$SITE/rpm/repodata"
createrepo_c "$SITE/rpm"
"${gpg_bin[@]}" --detach-sign --armor "$SITE/rpm/repodata/repomd.xml"

# ----------------------------------------------------------------------- pacman
(
  cd "$SITE/arch"
  rm -f dev-prompt.db* dev-prompt.files*
  if command -v repo-add >/dev/null; then
    repo-add dev-prompt.db.tar.gz ./*.pkg.tar.zst
  else
    # `repo-add` ships in the `pacman` package, already present in the image.
    docker run --rm -v "$PWD:/a" -w /a archlinux:latest \
      repo-add dev-prompt.db.tar.gz ./*.pkg.tar.zst
  fi
  # GitHub Pages doesn't serve symlinks — materialise the names pacman fetches
  # ("$repo.db", "$repo.files") as real files.
  for n in db files; do
    cp --remove-destination "dev-prompt.$n.tar.gz" "dev-prompt.$n"
  done
  # detached sigs under the fetched names: the db + every package
  for f in dev-prompt.db ./*.pkg.tar.zst; do
    "${gpg_bin[@]}" --detach-sign --no-armor -o "$f.sig" "$f"
  done
)

# --------------------------------------------------------------- key + landing
cp "$repo_root/packaging/repo/dev-prompt-repo.asc" "$SITE/dev-prompt.asc"
# Flatpak remote descriptors. The OSTree repo itself under $SITE/flatpak is put
# there by repo.yml (from the flatpak-repo job's artifact); these just point at it.
cp "$repo_root/packaging/flatpak/dev-prompt.flatpakrepo" "$SITE/dev-prompt.flatpakrepo"
cp "$repo_root/packaging/flatpak/io.github.seraphx2.devprompt.flatpakref" \
   "$SITE/io.github.seraphx2.devprompt.flatpakref"
KEYID="$KEYID" "$repo_root/packaging/repo/render-index.sh" > "$SITE/index.html"
# Pages: don't run the output through Jekyll
: > "$SITE/.nojekyll"

echo "repo rebuilt under $SITE"
