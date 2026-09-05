# Phase 6 — official distro repositories

**Lift:** none you control. **Status:** passive — this happens *to* the project
once the earlier phases make it adoptable.

## Reality

You cannot "submit to" Debian main, Fedora, or Arch `extra` the way you submit
to Flathub. Each requires a **distro-side maintainer** (often a Debian Developer
/ Fedora packager / Arch Trusted User) to sponsor and maintain the package,
following that distro's freeze schedule, packaging policy, and review. The
version in-repo will lag upstream, sometimes by a lot.

## What actually gets you there

1. **A clean release tarball.** `git archive` of a tag, or the GitHub
   auto-tarball, that builds with a documented, vendored dependency set
   (`cargo vendor`, committed or in the tarball). Distro build systems have no
   network.
2. **Bundled-dependency hygiene.** Distros dislike vendored Rust crates; be
   ready to explain the `Cargo.lock` and which crates are patched, if any.
3. **AppStream metainfo + a stable app-id** (Phase 1) — required for GNOME
   Software / Discover to show it once packaged.
4. **An existing AUR package** (Phase 2) is the usual proving ground and the
   thing an Arch TU looks at before promoting to `extra`.
5. **A Flathub presence** (Phase 4) signals the app is packaged responsibly.
6. **Licensing clarity.** MIT throughout, `LICENSE` present, no
   non-redistributable assets.

## When it's worth chasing actively

Only after there's real user demand on a given distro and phases 1–4 are done.
At that point: open a Debian RFP/ITP bug, a Fedora package review request, or
ping an Arch TU — with the tarball, the metainfo, and the AUR package as
evidence.

## Definition of done

There isn't one you can tick off. Track it as: "packaged in <distro> by
<maintainer>, version <x>" rows in `docs/linux-distribution/README.md` as they
appear.
