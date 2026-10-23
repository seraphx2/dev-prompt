#!/usr/bin/env bash
# Build throwaway repositories, one per working-copy state the repo list's
# status flags, the "!" attention list, the action menu's version-control
# section and the branch switcher (Ctrl+B) know about — for every system
# dev-prompt reads. Point it at one of your roots; every folder it makes
# starts with `_<system>-` (`_git-`, `_jj-`, `_hg-`, `_svn-`, `_fossil-`,
# `_brz-`), so they sort to the top of the list and `_jj` in the search box
# shows just that system's.
#
#   scripts/vcs-scenarios.sh /d/git                 # all systems installed
#   scripts/vcs-scenarios.sh ~/code jj hg           # just these
#   scripts/vcs-scenarios.sh ~/code all --reset     # delete them, build again
#   scripts/vcs-scenarios.sh ~/code --remove        # delete them all, and the remotes
#   scripts/vcs-scenarios.sh ~/code hg --remove     # just one system's
#
# Remotes live where the scan won't list them: Git's are bare repositories in
# `_git-remotes/`; everyone else's are in `_vcs-remotes/`, itself a Git repo
# ignoring its contents, so the Mercurial and Breezy repos inside collapse
# into it. Fetch, pull, push, sync and update all work offline — except
# `_git-github` and `_jj-github`, whose fetch really goes to GitHub.
# A system whose tool isn't installed is skipped, with a note.
set -euo pipefail

root="${1:?usage: vcs-scenarios.sh <root folder> [git jj hg svn fossil brz | all] [--reset | --remove]}"
shift
reset=""
remove=""
systems=()
for a in "$@"; do
  case "$a" in
    --reset) reset=1 ;;
    --remove) remove=1 ;;
    all) systems+=(git jj hg svn fossil brz) ;;
    git | jj | hg | svn | fossil | brz) systems+=("$a") ;;
    *) echo "unknown argument: $a" >&2; exit 2 ;;
  esac
done
[[ ${#systems[@]} -eq 0 ]] && systems=(git jj hg svn fossil brz)
root="$(cd "$root" && pwd)"

# The folders each system's scenarios make, for building and for --remove.
NAMES_git=(_git-remotes _git-clean _git-dirty _git-ahead _git-behind _git-diverged _git-unpublished _git-stash _git-detached _git-rebase-conflict _git-merge-conflict _git-no-remote _git-gone-upstream _git-worktrees _git-worktrees-feature-search _git-github)
NAMES_jj=(_jj-clean _jj-working _jj-ahead _jj-beyond _jj-unpublished _jj-conflict _jj-conflicted-change _jj-divergent _jj-no-bookmark _jj-workspaces _jj-workspaces-feature-search _jj-stale _jj-github)
NAMES_hg=(_hg-clean _hg-dirty _hg-drafts _hg-bookmarks _hg-rebase-conflict _hg-merge-conflict _hg-shelved _hg-no-remote)
NAMES_svn=(_svn-trunk _svn-branch _svn-conflict _svn-locked)
NAMES_fossil=(_fossil-clean _fossil-dirty _fossil-unsent _fossil-merge-conflict _fossil-branches _fossil-branches-feature-search _fossil-stash _fossil-no-remote)
NAMES_brz=(_brz-trunk _brz-feature _brz-conflict _brz-dirty)

# Native paths for the tools that don't understand Git Bash's /d/… form.
native() { if command -v cygpath >/dev/null; then cygpath -m "$1"; else printf '%s' "$1"; fi; }
file_url() { local p; p="$(native "$1")"; [[ "$p" == /* ]] && printf 'file://%s' "$p" || printf 'file:///%s' "$p"; }

# Fixed identities and no user configuration, so this behaves the same on
# any machine.
export GIT_AUTHOR_NAME="dev-prompt scenarios" GIT_AUTHOR_EMAIL="scenarios@example.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0
export HGUSER="dev-prompt scenarios <scenarios@example.invalid>" HGPLAIN=1 HGRCPATH=""
export JJ_USER="dev-prompt scenarios" JJ_EMAIL="scenarios@example.invalid"
export BRZ_EMAIL="dev-prompt scenarios <scenarios@example.invalid>"
export USER="scenarios" # Fossil's identity
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
jjconf="$scratch/jj-config.toml"
: >"$jjconf"
export JJ_CONFIG="$jjconf"

q() { "$@" >/dev/null 2>&1; }
days_ago() { echo "$(( $(date +%s) - $1 * 86400 ))"; }

# fresh <names…> — refuse to touch folders that exist, unless --reset.
fresh() {
  for n in "$@"; do
    if [[ -n "$reset" ]]; then rm -rf "${root:?}/$n"; fi
    if [[ -e "$root/$n" ]]; then
      echo "$root/$n already exists — run again with --reset to rebuild." >&2
      exit 1
    fi
  done
}

# The shared home of every non-Git remote: a Git repo that ignores everything
# in it, so repos inside collapse into one clean, local-only entry.
vcs_remotes() {
  local r="$root/_vcs-remotes"
  if [[ ! -d "$r/.git" ]]; then
    mkdir -p "$r"
    git init -q --initial-branch=main "$r"
    printf '*\n!.gitignore\n!README.md\n' >"$r/.gitignore"
    printf '# Remotes for the dev-prompt scenario repos\n\nMade by scripts/vcs-scenarios.sh; delete with the _*- folders.\n' >"$r/README.md"
    (cd "$r" && git add .gitignore README.md && git commit -q -m "remotes for the scenario repos")
  fi
  printf '%s' "$r"
}

made=()

# --- Git -------------------------------------------------------------------------

build_git() {
  local names=("${NAMES_git[@]}")
  fresh "${names[@]}"

  # commit <file> <text> [days ago] — write a file and commit it.
  commit() {
    printf '%s\n' "$2" >"$1"
    git add -- "$1"
    local d; d="$(days_ago "${3:-0}") +0000"
    GIT_AUTHOR_DATE="$d" GIT_COMMITTER_DATE="$d" git commit -q -m "$1: $2"
  }
  # repo <name> — a bare remote plus a clone of it with a first commit pushed.
  repo() {
    git init -q --bare --initial-branch=main "$root/_git-remotes/$1.git"
    git clone -q "$root/_git-remotes/$1.git" "$root/$1" 2>/dev/null
    cd "$root/$1"
    git checkout -q -B main
    commit README.md "# $1" 10
    commit app.txt "first version" 9
    git push -q -u origin main
  }
  # someone_else <name> <count> — commits pushed to the remote from elsewhere.
  someone_else() {
    rm -rf "$scratch/other"
    git clone -q "$root/_git-remotes/$1.git" "$scratch/other"
    (cd "$scratch/other" && for i in $(seq 1 "$2"); do commit "theirs-$i.txt" "their change $i" 1; done && git push -q)
  }
  mkdir -p "$root/_git-remotes"

  # Up to date with origin/main: no flags, not in the attention list.
  repo _git-clean

  # Uncommitted work (~2 changed, +1 new). The branch carries a ticket number:
  # type PROJ-1234 in the search box to find every repo on that ticket.
  repo _git-dirty
  git checkout -q -b PROJ-1234-invoice-api
  git push -q -u origin PROJ-1234-invoice-api
  printf 'edited\n' >>README.md
  printf 'edited\n' >>app.txt
  printf 'scratch\n' >notes.txt

  # Two commits not pushed (↑2): the menu offers "Push 2 commits to …".
  repo _git-ahead
  git checkout -q -b PROJ-1234-invoice-export
  git push -q -u origin PROJ-1234-invoice-export
  commit export.txt "csv export" 1
  commit export.txt "csv export, with totals" 0

  # Three commits on the remote, already fetched (↓3): "Pull 3 commits …".
  repo _git-behind
  someone_else _git-behind 3
  git fetch -q

  # Ahead and behind at once: neither a fast-forward pull nor a plain push
  # applies, so neither is offered — that's a job for a git GUI.
  repo _git-diverged
  someone_else _git-diverged 2
  git fetch -q
  commit mine.txt "my change" 0

  # A branch with commits but no upstream: "Publish feature/login to origin".
  repo _git-unpublished
  git checkout -q -b feature/login
  commit login.txt "login form" 2
  commit login.txt "login form, validated" 1

  # Clean, with two stashes (≡2).
  repo _git-stash
  printf 'half an idea\n' >>app.txt && q git stash push -m "half an idea"
  printf 'another idea\n' >>README.md && q git stash push -m "another idea"

  # HEAD detached at an older commit.
  repo _git-detached
  q git checkout --detach HEAD~1

  # A rebase stopped on a conflict: "rebasing", !1.
  repo _git-rebase-conflict
  git checkout -q -b topic
  commit app.txt "topic's version" 3
  git push -q -u origin topic
  git checkout -q main
  commit app.txt "main's version" 2
  git push -q
  git checkout -q topic
  q git rebase main || true

  # A merge stopped on a conflict: "merging", !1.
  repo _git-merge-conflict
  git checkout -q -b other
  commit app.txt "other's version" 3
  git checkout -q main
  commit app.txt "main's version" 2
  git push -q
  q git merge other || true

  # No remote at all: shows "local", but isn't in the attention list — a local
  # repo is a choice. (Its history exists only here, which the Recycle Bin
  # dialog does count.)
  git init -q --initial-branch=main "$root/_git-no-remote"
  cd "$root/_git-no-remote"
  commit notes.md "local notes" 5

  # The upstream branch was deleted on the remote, and a fetch pruned it: "gone".
  repo _git-gone-upstream
  git checkout -q -b feature/merged-already
  commit feature.txt "done" 4
  git push -q -u origin feature/merged-already
  git push -q origin --delete feature/merged-already
  git fetch -q --prune

  # Branches to switch between (Ctrl+B): several local ones checked out in
  # turn, branches only on the remote, and one checked out in a linked worktree
  # beside it (_git-worktrees-feature-search — Enter on it goes there).
  repo _git-worktrees
  for b in feature/search feature/settings-page fix/crash-on-empty chore/deps; do
    git checkout -q -b "$b" main
    commit "${b//\//-}.txt" "work on $b" "$(( RANDOM % 20 + 1 ))"
    git push -q -u origin "$b"
  done
  git checkout -q main
  git checkout -q fix/crash-on-empty
  git checkout -q feature/settings-page
  git checkout -q main
  someone_else _git-worktrees 1
  (cd "$scratch/other" && git checkout -q -b release/v2 && commit release.txt "release notes" 3 && git push -q -u origin release/v2 \
    && git checkout -q -b experiment/new-parser main && commit parser.txt "a faster parser" 6 && git push -q -u origin experiment/new-parser)
  git fetch -q
  git worktree add -q "$root/_git-worktrees-feature-search" feature/search

  # A GitHub remote: "Open on GitHub", the branch's page, "Create pull request".
  # The remote-tracking refs are made locally so no network is needed until you
  # fetch (which then talks to github.com, and prunes the made-up branch).
  git init -q --initial-branch=main "$root/_git-github"
  cd "$root/_git-github"
  commit README.md "# a repo on GitHub" 3
  git remote add origin https://github.com/seraphx2/dev-prompt.git
  git update-ref refs/remotes/origin/main HEAD
  git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  git branch -q -u origin/main main
  git checkout -q -b feature/branch-status
  commit status.txt "branch status" 1
  git update-ref refs/remotes/origin/feature/branch-status HEAD
  git branch -q -u origin/feature/branch-status feature/branch-status

  made+=("${names[@]}")
}

# --- Jujutsu ----------------------------------------------------------------------

build_jj() {
  local names=("${NAMES_jj[@]}")
  fresh "${names[@]}"
  local remotes; remotes="$(vcs_remotes)"
  # jj <args> in the current folder, quietly.
  j() { jj --no-pager "$@" >/dev/null 2>&1; }
  # change <file> <text> <description> — a change with content, then a new
  # empty one on top (`jj commit`).
  change() { printf '%s\n' "$2" >"$1"; j commit -m "$3"; }
  # repo <name> — a bare Git remote, and a jj repo with `main` pushed to it.
  repo() {
    rm -rf "$remotes/jj-$1.git"
    git init -q --bare --initial-branch=main "$remotes/jj-$1.git"
    mkdir -p "$root/$1"
    cd "$root/$1"
    j git init
    j git remote add origin "$(native "$remotes/jj-$1.git")"
    change README.md "# $1" "readme"
    change app.txt "first version" "first version"
    j bookmark create main -r @-
    j git push --bookmark main
  }

  # main pushed, an empty change on top: no flags.
  repo _jj-clean

  # Edits in the working-copy change (~2 +…): jj counts every changed file.
  repo _jj-working
  printf 'edited\n' >>README.md
  printf 'new\n' >notes.txt

  # main moved ahead of main@origin by two (↑2): "Push main to origin".
  repo _jj-ahead
  change export.txt "csv export" "csv export"
  change export.txt "with totals" "csv export, with totals"
  j bookmark move main --to @-

  # Two changes on top of main, the bookmark left where it was: "main +2".
  repo _jj-beyond
  change a.txt "one" "first idea"
  change b.txt "two" "second idea"

  # A bookmark with no remote counterpart: "Publish feature/login to origin".
  repo _jj-unpublished
  change login.txt "login form" "login form"
  j bookmark create feature/login -r @-

  # A conflict in the working-copy change (!1).
  repo _jj-conflict
  j new main -m x
  printf 'x\n' >app.txt
  j new main -m y
  printf 'y\n' >app.txt
  j rebase -r @ -d 'description(substring:"x")'

  # A conflict left in a change of its own, the working copy moved off it:
  # "!1ch" — jj records conflicts rather than stopping.
  repo _jj-conflicted-change
  j new main -m x
  printf 'x\n' >app.txt
  j new main -m y
  printf 'y\n' >app.txt
  j rebase -r @ -d 'description(substring:"x")'
  j new main

  # The same change rewritten in two places at once: "divergent".
  repo _jj-divergent
  j describe -m "one way"
  local op
  op="$(jj --no-pager op log --no-graph -T 'id.short() ++ "\n"' -n 2 | sed -n 2p)"
  j --at-op "$op" describe -m "the other way"
  j log -r @

  # No bookmark anywhere below the working copy, no remote: the change's own
  # id stands in for a branch, which is how jj is mostly used.
  mkdir -p "$root/_jj-no-bookmark"
  cd "$root/_jj-no-bookmark"
  j git init
  change notes.md "local notes" "notes"

  # Bookmarks to switch to (Ctrl+B: a new change on top of the one picked),
  # one only on the remote, and one under a second workspace beside it.
  repo _jj-workspaces
  for b in feature/search feature/settings fix/crash; do
    j new main
    change "${b//\//-}.txt" "work on $b" "work on $b"
    j bookmark create "$b" -r @-
    j git push --bookmark "$b"
  done
  j new main
  change release.txt "release notes" "release notes"
  j bookmark create release/v2 -r @-
  j git push --bookmark release/v2
  j bookmark forget release/v2
  j git fetch
  j new main
  j workspace add "$(native "$root/_jj-workspaces-feature-search")" --name feature-search -r feature/search

  # A working copy moved while jj wasn't looking at its files (here, a
  # command run with --ignore-working-copy; usually another workspace or a
  # concurrent command): "stale" until `jj workspace update-stale`.
  repo _jj-stale
  change notes.txt "more" "more notes"
  j --ignore-working-copy new main

  # A GitHub remote, colocated with Git so its refs can be made locally: the
  # web links and a pull request for feature/branch-status.
  mkdir -p "$root/_jj-github"
  cd "$root/_jj-github"
  j git init --colocate
  change README.md "# a repo on GitHub" "readme"
  j bookmark create main -r @-
  git remote add origin https://github.com/seraphx2/dev-prompt.git
  git update-ref refs/remotes/origin/main "$(jj --no-pager log --no-graph -r main -T commit_id)"
  change status.txt "branch status" "branch status"
  j bookmark create feature/branch-status -r @-
  git update-ref refs/remotes/origin/feature/branch-status "$(jj --no-pager log --no-graph -r feature/branch-status -T commit_id)"
  j git import
  j bookmark track main@origin feature/branch-status@origin

  made+=("${names[@]}")
}

# --- Mercurial --------------------------------------------------------------------

build_hg() {
  local names=("${NAMES_hg[@]}")
  fresh "${names[@]}"
  local remotes; remotes="$(vcs_remotes)"
  h() { hg --config extensions.rebase= --config extensions.shelve= "$@" >/dev/null 2>&1; }
  commit() {
    printf '%s\n' "$2" >"$1"
    h add "$1" || true
    h commit -m "$1: $2" -d "$(days_ago "${3:-0}") 0"
  }
  repo() {
    rm -rf "$remotes/hg-$1"
    h init "$remotes/hg-$1"
    h clone "$remotes/hg-$1" "$root/$1"
    cd "$root/$1"
    commit README.md "# $1" 10
    commit app.txt "first version" 9
    h push
  }

  repo _hg-clean

  repo _hg-dirty
  printf 'edited\n' >>app.txt
  printf 'scratch\n' >notes.txt

  # Two draft commits — not yet pushed, which Mercurial knows offline:
  # "Push 2 draft commits to default".
  repo _hg-drafts
  commit export.txt "csv export" 1
  commit export.txt "with totals" 0

  # Bookmarks and a named branch to switch between (Ctrl+B).
  repo _hg-bookmarks
  h branch stable
  commit app.txt "stable fix" 6
  h update default
  for b in feature-search feature-settings fix-crash; do
    h update default
    h bookmark "$b"
    commit "$b.txt" "work on $b" "$(( RANDOM % 9 + 1 ))"
  done
  h update feature-search

  # A rebase stopped on a conflict: "rebasing", !1.
  repo _hg-rebase-conflict
  h bookmark topic
  commit app.txt "topic's version" 3
  h update -r 1
  commit app.txt "default's version" 2
  h update topic
  h rebase -d tip || true

  # A merge stopped on a conflict: "merging", !1.
  repo _hg-merge-conflict
  commit app.txt "one side" 3
  h update -r 1
  commit app.txt "other side" 2
  h merge || true

  # Two shelved changes (≡2).
  repo _hg-shelved
  printf 'half an idea\n' >>app.txt && h shelve --name idea-one
  printf 'another\n' >>README.md && h shelve --name idea-two

  # No remote: "local".
  h init "$root/_hg-no-remote"
  cd "$root/_hg-no-remote"
  commit notes.md "local notes" 5

  made+=("${names[@]}")
}

# --- Subversion ---------------------------------------------------------------------

build_svn() {
  local names=("${NAMES_svn[@]}")
  fresh "${names[@]}"
  local remotes; remotes="$(vcs_remotes)"
  local repo="$remotes/svn-repo"
  rm -rf "$repo"
  svnadmin create "$(native "$repo")"
  local url; url="$(file_url "$repo")"
  s() { svn --non-interactive "$@" >/dev/null 2>&1; }
  s mkdir -m "layout" "$url/trunk" "$url/branches" "$url/tags"
  s checkout "$url/trunk" "$(native "$scratch/svn-seed")"
  (cd "$scratch/svn-seed" && printf '# project\n' >README.md && printf 'first\n' >app.txt && s add README.md app.txt && s commit -m "first version")
  for b in feature-x release-2 old-experiment; do s copy -m "branch $b" "$url/trunk" "$url/branches/$b"; done

  # trunk, clean; Ctrl+B lists the branches (asking the server) and makes
  # new ones there; "Update from the server" is always on offer.
  s checkout "$url/trunk" "$(native "$root/_svn-trunk")"

  # On branches/feature-x with local work (~1 +1): the switcher asks for a
  # commit (or revert) first, since svn switch would merge it in.
  s checkout "$url/branches/feature-x" "$(native "$root/_svn-branch")"
  cd "$root/_svn-branch"
  printf 'edited\n' >>app.txt
  printf 'scratch\n' >notes.txt

  # An update that ran into a local edit: a conflict, postponed (!1).
  s checkout "$url/trunk" "$(native "$root/_svn-conflict")"
  (cd "$scratch/svn-seed" && s update && printf 'theirs\n' >app.txt && s commit -m "their change")
  cd "$root/_svn-conflict"
  printf 'mine\n' >app.txt
  s update --accept postpone || true

  # A working copy an interrupted command left locked: "needs cleanup" until
  # `svn cleanup`. (Made by recording the lock an aborted command leaves.)
  s checkout "$url/trunk" "$(native "$root/_svn-locked")"
  if command -v python >/dev/null || command -v python3 >/dev/null; then
    "$(command -v python3 || command -v python)" - "$(native "$root/_svn-locked/.svn/wc.db")" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute("INSERT INTO WC_LOCK (wc_id, local_dir_relpath, locked_levels) VALUES (1, '', -1)")
db.commit()
PY
  else
    echo "  (no Python here, so _svn-locked isn't locked)" >&2
  fi

  made+=("${names[@]}")
}

# --- Fossil ----------------------------------------------------------------------------

build_fossil() {
  local names=("${NAMES_fossil[@]}")
  fresh "${names[@]}"
  local remotes; remotes="$(vcs_remotes)"
  f() { fossil "$@" >/dev/null 2>&1; }
  commit() { printf '%s\n' "$2" >"$1"; f add "$1" || true; f commit -m "$1: $2" --no-warnings "${@:3}"; }
  # repo <name> — a repository file in the remotes folder, cloned (so it has
  # a remote) and opened as <name>.
  repo() {
    local src="$remotes/fossil-$1.fossil"
    rm -f "$src" "$remotes/fossil-$1-clone.fossil"
    f init "$(native "$src")"
    f clone "$(native "$src")" "$(native "$remotes/fossil-$1-clone.fossil")"
    mkdir -p "$root/$1"
    cd "$root/$1"
    f open "$(native "$remotes/fossil-$1-clone.fossil")"
    commit README.md "# $1"
    commit app.txt "first version"
  }

  repo _fossil-clean

  repo _fossil-dirty
  printf 'edited\n' >>app.txt
  printf 'scratch\n' >notes.txt

  # Autosync off and two check-ins since: unsent (↑2), "Sync with …".
  repo _fossil-unsent
  f settings autosync off
  commit export.txt "csv export"
  commit export.txt "with totals"

  # A merge with a conflict, not yet committed: "merging", !1.
  repo _fossil-merge-conflict
  commit app.txt "feature's version" --branch feature
  f update trunk
  commit app.txt "trunk's version"
  f merge feature || true

  # Branches to switch between (Ctrl+B), and a second checkout of the same
  # repository on one of them, beside it.
  repo _fossil-branches
  for b in feature-search feature-settings fix-crash; do
    f update trunk
    commit "$b.txt" "work on $b" --branch "$b"
  done
  f update trunk
  mkdir -p "$root/_fossil-branches-feature-search"
  (cd "$root/_fossil-branches-feature-search" && f open "$(native "$remotes/fossil-_fossil-branches-clone.fossil")" feature-search)

  repo _fossil-stash
  printf 'half an idea\n' >>app.txt
  f stash save -m "half an idea"

  # No remote: "local".
  mkdir -p "$root/_fossil-no-remote"
  cd "$root/_fossil-no-remote"
  f init "$(native "$remotes/fossil-no-remote.fossil")"
  f open "$(native "$remotes/fossil-no-remote.fossil")"
  f settings autosync off
  commit notes.md "local notes"

  made+=("${names[@]}")
}

# --- Breezy ----------------------------------------------------------------------------

build_brz() {
  local names=("${NAMES_brz[@]}")
  fresh "${names[@]}"
  local remotes; remotes="$(vcs_remotes)"
  # Breezy on Windows resolves relative file arguments against the drive
  # root, so everything gets a full path or none.
  b() { brz "$@" >/dev/null 2>&1; }
  commit() { printf '%s\n' "$2" >"$1"; b add; b commit -m "$1: $2"; }

  # The parent branch.
  b init "$(native "$root/_brz-trunk")"
  cd "$root/_brz-trunk"
  commit README.md "# trunk"
  commit app.txt "first version"

  # A branch of it, pushed once to a remembered location: "Push to …"; then
  # trunk moves on, so "Pull from …_brz-trunk" brings it in.
  b branch "$(native "$root/_brz-trunk")" "$(native "$root/_brz-feature")"
  rm -rf "$remotes/brz-feature-mirror"
  (cd "$root/_brz-feature" && b push --remember "$(native "$remotes/brz-feature-mirror")")
  cd "$root/_brz-trunk"
  commit app.txt "second version"
  commit notes.txt "release notes"

  # A merge that conflicted, not yet committed: "merging", !1.
  b branch "$(native "$root/_brz-trunk")" "$(native "$root/_brz-conflict")"
  cd "$root/_brz-conflict"
  commit app.txt "conflict's version"
  b merge "$(native "$root/_brz-feature")" || true
  (cd "$root/_brz-feature" && commit app.txt "feature's version")
  b revert
  b merge "$(native "$root/_brz-feature")" || true

  # Uncommitted work and a shelved change (~1 +1 ≡1).
  b branch "$(native "$root/_brz-trunk")" "$(native "$root/_brz-dirty")"
  cd "$root/_brz-dirty"
  printf 'half an idea\n' >>app.txt
  b shelve --all -m "half an idea"
  printf 'edited\n' >>README.md
  printf 'scratch\n' >scratch.txt

  made+=("${names[@]}")
}

# --- remove ----------------------------------------------------------------------------

# Delete the chosen systems' folders. With every system chosen, the shared
# remotes go too (`_git-remotes`, `_vcs-remotes`); otherwise only those
# systems' own remotes inside `_vcs-remotes`. Only folders this script makes,
# by name — nothing else in the root is touched.
remove_scenarios() {
  local gone=0 every=1
  for s in git jj hg svn fossil brz; do
    [[ " ${systems[*]} " == *" $s "* ]] || every=""
  done
  for s in "${systems[@]}"; do
    local -n list="NAMES_$s"
    for n in "${list[@]}"; do
      if [[ -e "$root/$n" ]]; then
        # Forget Fossil checkouts in Fossil's own list of them, too.
        if [[ "$s" == fossil ]] && command -v fossil >/dev/null; then
          (cd "$root/$n" && fossil close --force >/dev/null 2>&1) || true
        fi
        rm -rf "${root:?}/$n"
        gone=$((gone + 1))
      fi
    done
    # This system's remotes in the shared folder.
    if [[ -d "$root/_vcs-remotes" ]]; then
      case "$s" in
        jj) rm -rf "$root"/_vcs-remotes/jj-* ;;
        hg) rm -rf "$root"/_vcs-remotes/hg-* ;;
        svn) rm -rf "$root"/_vcs-remotes/svn-repo ;;
        fossil) rm -rf "$root"/_vcs-remotes/fossil-* ;;
        brz) rm -rf "$root"/_vcs-remotes/brz-* ;;
      esac
    fi
  done
  if [[ -n "$every" ]]; then
    for n in _git-remotes _vcs-remotes; do
      if [[ -e "$root/$n" ]]; then rm -rf "${root:?}/$n"; gone=$((gone + 1)); fi
    done
  fi
  echo "Removed $gone folders from $root."
}

if [[ -n "$remove" ]]; then
  remove_scenarios
  exit 0
fi

# --- run -------------------------------------------------------------------------------

need() { command -v "$1" >/dev/null || { echo "Skipping $2: \`$1\` isn't on PATH." >&2; return 1; }; }
for s in "${systems[@]}"; do
  case "$s" in
    git) need git Git && build_git ;;
    jj) need jj Jujutsu && need git "Jujutsu (its remotes are Git)" && build_jj ;;
    hg) need hg Mercurial && build_hg ;;
    svn) need svn Subversion && need svnadmin Subversion && build_svn ;;
    fossil) need fossil Fossil && build_fossil ;;
    brz) need brz Breezy && build_brz ;;
  esac
done
echo "Made ${#made[@]} folders in $root."
