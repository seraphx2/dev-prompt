# 02 — Repo pipeline (`build-repo.sh`, `dependabot.yml`)

Two low-severity / housekeeping findings in the self-hosted repo publish path.

---

## R0907-5 — `build-repo.sh` rpm signing loop aborts on an empty pool

```sh
shopt -s nullglob                                    # line 36
for f in "$ARTIFACTS"/*.deb;         do cp -f ...; done
for f in "$ARTIFACTS"/*.rpm;         do cp -f ...; done
for f in "$ARTIFACTS"/*.pkg.tar.zst; do cp -f ...; done
shopt -u nullglob                                    # line 40
...
for f in "$SITE"/rpm/*.rpm; do                       # line 88 — nullglob OFF
  rpm --define "_gpg_name $KEYID" \
      --define "__gpg_sign_cmd $rpm_sign_cmd" \
      --addsign "$f"
done
```

The ingest loops are inside the `nullglob` region, so an empty artifact set is a
no-op there. The `--addsign` loop at line 88 is **outside** it: with no `.rpm` in
`$SITE/rpm/`, `f` takes the literal value `<SITE>/rpm/*.rpm`, `rpm --addsign`
fails with "No such file or directory", and `set -euo pipefail` kills the whole
script — the `publish` job fails and nothing (deb/rpm/pacman/flatpak) is
published.

**Currently latent:** the live `gh-pages` `rpm/` pool has packages from past
releases, so the glob expands. It bites when:

- `gh-pages` is rebuilt from scratch (branch deleted, disaster recovery), or
- `gh release download -p '*.x86_64.rpm'` matches nothing — `gh release
  download` does **not** error when only some `-p` patterns match, so an rpm
  naming change or `bundle.targets` edit that drops rpm would leave `$ARTIFACTS`
  (and a fresh `$SITE/rpm/`) empty.

The deb path already tolerates this; the rpm path should match.

**Fix:** wrap the loop in `shopt -s nullglob` / `shopt -u nullglob`, or guard the
body with `[ -e "$f" ] || continue`. Same treatment for the
`for f in dev-prompt.db ./*.pkg.tar.zst` loop at line ~114 (secondary — `repo-add`
would have already failed on an empty arch pool).

---

## R0907-6 — `dependabot.yml` `github-actions` block missing `target-branch`

```yaml
- package-ecosystem: github-actions
  directory: "/"
  schedule: { interval: weekly }
  # no target-branch
  ...
- package-ecosystem: npm
  directory: "/"
  target-branch: dev          # <-- npm and cargo set it
- package-ecosystem: cargo
  directory: "/src-tauri"
  target-branch: dev
```

**Not a bug today:** the repo's default branch is `dev`, so the `github-actions`
block (no `target-branch`) already opens PRs against `dev` — same as the other
two. Confirmed in practice: PR #15 (the actions group) opened against `dev` and
merged cleanly.

The gap is future-proofing. The header comment states "PRs target `dev` — the
default branch and where CI (`ci-dev.yml`) runs", and `816fbbf` set `main` as
release-on-merge. If the default is ever flipped to `main`, `npm` / `cargo` stay
pinned to `dev` while `github-actions` bumps would start landing on the release
branch, routing around the `dev` gate.

**Fix:** add `target-branch: dev` to the `github-actions` block so all three are
explicit and consistent.
