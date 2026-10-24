#!/usr/bin/env bash
# Take the screenshots under Linux from Windows, through WSL — the fonts then
# match the AppStream shots, as capture.mjs asks. Run it from Windows:
#
#   npm run screenshots:wsl                    # every image
#   npm run screenshots:wsl -- --only 8,9      # extra args go to capture.mjs
#
# Everything it needs goes in the WSL user's home, no sudo: Node (nodejs.org's
# Linux build, checksum-checked) in ~/.local/node, Chromium's NSS libraries
# (Ubuntu's own packages, unpacked) in ~/.local/chromium-libs, and a Linux copy
# of the working tree with Linux node_modules in ~/dev-prompt-shots. The
# images come back into this checkout's docs/img/.
set -euo pipefail

src="$(cd "$(dirname "$0")/../.." && pwd)"
dst="$HOME/dev-prompt-shots"
node_dir="$HOME/.local/node"
libs_dir="$HOME/.local/chromium-libs"
export PATH="$node_dir/bin:$PATH"

if ! command -v node >/dev/null; then
  echo "Installing Node 22 LTS into $node_dir…"
  base=https://nodejs.org/dist/latest-v22.x
  sums=$(curl -fsSL "$base/SHASUMS256.txt")
  f=$(printf '%s\n' "$sums" | grep -oE 'node-v[0-9.]+-linux-x64\.tar\.xz' | head -1)
  tmp=$(mktemp -d)
  (cd "$tmp" && curl -fsSLO "$base/$f" && printf '%s\n' "$sums" | grep " $f\$" | sha256sum -c - >/dev/null)
  mkdir -p "$node_dir"
  tar -xJf "$tmp/$f" -C "$node_dir" --strip-components=1
  rm -rf "$tmp"
fi

# A copy of the working tree's sources; node_modules stays the Linux one.
mkdir -p "$dst"
(cd "$src" && tar --exclude=./node_modules --exclude=./src-tauri/target --exclude=./dist --exclude=./.git -cf - .) | (cd "$dst" && tar -xf -)
cd "$dst"
if ! cmp -s package-lock.json node_modules/.package-lock-copy 2>/dev/null; then
  echo "Installing packages…"
  npm ci --no-audit --no-fund --loglevel=error
  cp package-lock.json node_modules/.package-lock-copy
fi
npx playwright install chromium >/dev/null

# Chromium needs NSS, which a minimal Ubuntu lacks; `apt-get download` gets
# the distribution's own packages without root.
shell=$(find "$HOME/.cache/ms-playwright" -type f -name headless_shell | head -1)
lib_path=$(find "$libs_dir" -name libnss3.so -printf '%h\n' 2>/dev/null | head -1 || true)
if ldd "$shell" | grep -q "not found" && [ -z "$lib_path" ]; then
  echo "Fetching Chromium's NSS libraries into $libs_dir…"
  tmp=$(mktemp -d)
  (cd "$tmp" && apt-get download libnss3 libnspr4 >/dev/null)
  mkdir -p "$libs_dir"
  for d in "$tmp"/*.deb; do dpkg-deb -x "$d" "$libs_dir"; done
  rm -rf "$tmp"
  lib_path=$(find "$libs_dir" -name libnss3.so -printf '%h\n' | head -1)
fi
if [ -n "$lib_path" ]; then export LD_LIBRARY_PATH="$lib_path"; fi
if ldd "$shell" | grep -q "not found"; then
  echo "Chromium still can't find:" >&2
  ldd "$shell" | grep "not found" >&2
  exit 1
fi

npm run screenshots -- "$@"
cp docs/img/*.png docs/img/*.gif "$src/docs/img/"
if ls docs/img/*.mp4 >/dev/null 2>&1; then cp docs/img/*.mp4 "$src/docs/img/"; fi
echo "Images copied into $src/docs/img/."
