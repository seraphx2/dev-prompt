# Rules engine

The rules engine is what turns a directory into a list of actions: discovery
markers, program resolution, manifest→action rules, and the always-offered
"universal" actions.

Its defaults are bundled in the binary
([`src-tauri/src/default_config.yaml`](../src-tauri/src/default_config.yaml) —
the canonical, inline-documented schema). You extend or override them in
**`rules.yaml`** in your OS config directory:

- Windows — `%APPDATA%\dev-prompt\rules.yaml`
- Linux — `~/.config/dev-prompt/rules.yaml`
- macOS — `~/Library/Application Support/dev-prompt/rules.yaml`

Open it from **Settings ▸ Rules ▸ Open rules file**; it ships as a commented
scaffold. After editing, hit **Settings ▸ Rules ▸ Reload config** (or restart) —
see [Applying changes](#applying-changes).

Your *settings* — `hotkey`, `apps_hotkey`, `roots`, `scan`, `cache_ttl_secs`,
`terminal`, `terminal_template`, `shell`, `apps` — are **not** here. They live in
`config.yaml` and are edited in the Settings screen; putting them in `rules.yaml`
has no effect. See [configuration.md](configuration.md).

---

## How the merge works

`rules.yaml` is layered over the bundled defaults, per section:

| Section | Strategy |
|---|---|
| `markers` | **Appended** to the defaults. Set `markers_replace: true` to use *only* your list. |
| `programs` | **Merged by key.** Your `code:` replaces the built-in `code:`; a new key is added. |
| `rules` | Your rules are **prepended** — they run first and show first. `rules_disable: [id]` sets a built-in aside. |
| `universal` | `universal.disable: [id]` removes built-ins, `universal.add: [...]` appends yours, `universal.default: id` sets which action Enter runs on a repo in the main list. |

A disabled built-in isn't deleted — it still appears in the Settings
"Active configuration" viewer marked *disabled*, it's just never evaluated. This
is so you can see what you turned off.

`rules.yaml` ships as a commented scaffold; uncomment the sections you need.
Anything omitted keeps the default.

---

## Building blocks

### `markers`

A filename or glob that makes a directory count as a project (so it shows up in
the list). Every rule's `match` also counts as a marker, so you rarely need to
add one unless it has no associated actions.

```yaml
markers:
  - .git
  - "*.sln"
  - { name: .hg, kind: vcs, label: Mercurial }   # kind: vcs → row badge
```

### `programs`

A named recipe for locating an executable, referenced from actions as
`{{key}}`. Candidates are tried in order; the first that resolves wins. Split by
OS — `any` is tried on every platform, then the current OS's list.

```yaml
programs:
  code:
    any: [code]                 # bare name → PATH lookup (PATHEXT-aware)
    windows: [code.cmd]
  vs:
    windows:
      - { vswhere: "-latest -property productPath" }   # run vswhere.exe, use stdout
      - "%ProgramFiles%/Microsoft Visual Studio/*/*/Common7/IDE/devenv.exe"  # glob
  anypoint:
    windows:
      - "D:/tools/AnypointStudio/AnypointStudio.exe"    # absolute path
```

Candidate forms: a **bare name** (looked up on `PATH`), an **absolute path**
(used if the file exists), a **glob** (first matching file), or
`{ vswhere: "<args>" }` (Windows — runs `vswhere.exe` and uses its output).
`~`, `%VAR%`, and `$VAR` are expanded.

### `rules`

Match a manifest, emit actions.

```yaml
rules:
  - id: maven                 # optional; used by rules_disable and action ids
    match: pom.xml            # string or list; globs allowed ("*.csproj")
    when: windows             # optional: windows | linux | macos | unix
    scope: project            # project (default) = run in the project dir
                              # repo = run at the repo root
    per_file: false           # true = one action set per matched file,
                              #   with {{file}} / {{file.stem}} bound
    requires: [mvn]           # rule hidden unless every binary is on PATH
    needs: [vs]               # rule hidden unless every program key resolves
    actions: [ ... ]          # see below
    # provider: npm-scripts   # instead of `actions`: a built-in generator
                              #   (npm-scripts | cargo | go | python | compose)
```

### `actions`

The list an action-based rule (or `universal`) emits.

```yaml
actions:
  - name: "Open {{file.stem}} in Visual Studio"
    program: "{{vs}}"          # a program key or literal; template-expanded
    args: ["{{file}}"]         # explicit argv
    needs: [vs]                # program keys (from `programs`) that must
                              #   resolve, or this action is hidden

  - name: "mvn package"
    run: "mvn -B package"      # OR a single string, quote-aware split
    terminal: true            # wrap in the terminal at the working dir

  - name: "Copy path"
    client: true              # handled in the frontend, no process (copy-path only)

  - name: "make"
    run: "make"
    terminal: true
    icon: run                 # glyph for the menu row — see Settings > Icons

  - name: "npm run…"
    run: "npm run {{input}}"  # prompt: opens the "Run command…" input;
    prompt: true              #   {{input}} = what you type, the rest is fixed.
    terminal: true            #   A bare `prompt: true` (no `run:`) takes a
                              #   whole command line. Blank + a shell = open it.
```

**`{{vs}}` and `needs: [vs]` refer to the same thing** — `vs` is a key in the
`programs` map. `{{vs}}` substitutes that program's resolved path into the
command; `needs: [vs]` hides the whole action when `vs` can't be found on this
machine. You usually list a key in both: one makes the command runnable, the
other stops a dead row from showing. If a key appears in `program:`, an
unresolved value already drops the action, so `needs:` there is just
belt-and-suspenders — it's load-bearing when the key is only in `args:` / `run:`,
or when the action has no program at all (a gated `terminal: true`).

- `program` + `args`, **or** `run` — not both.
- `terminal: true` runs it inside the resolved terminal at the working
  directory. Without it, the process is spawned detached with no window.
- `prompt: true` doesn't run anything — it opens the **Run command…** input in
  the action menu, seeded with `run:` as a template (`{{input}}` is where the
  typed text goes). The input has its own shell picker; leave it blank and pick
  a shell to just open that shell in the repo.
- `needs:` gates just this action. `needs:` / `requires:` on the *rule* gate the
  whole rule — and `requires:` is rule-only, taking bare executable names checked
  on `PATH` rather than `programs` keys.
- `client: true` is reserved for the built-in `copy-path`; custom client
  actions aren't wired up.
- `icon:` picks the row glyph. **Settings ▸ Icons** lists every bundled key
  (click one to copy `icon: <key>`); untagged actions fall back to a neutral
  glyph.

### `universal`

Actions offered for every repo, regardless of contents. Each entry uses the same
fields as [`actions`](#actions) above — `program` + `args` or `run`, `terminal`,
`needs` (`programs` keys), `client` — plus an `id` used by `universal.disable`
and `universal.default`.

```yaml
universal:
  default: terminal            # the action that runs when you press Enter on a
                               #   repo in the main list, without opening its
                               #   action menu (falls back to the first action)
  actions:
    - { id: terminal, name: "Open in terminal", terminal: true }
    - { id: vscode, name: "Open in VS Code", program: "{{code}}", args: ["{{path}}"], needs: [code] }
```

### Template variables

Usable in `name`, `program`, `args`, and `run`:

| | |
|---|---|
| `{{path}}` | the project directory (or repo root, for `scope: repo`) |
| `{{repo}}` | the repo root |
| `{{rel}}` | sub-project path relative to the repo (`""` at the root) |
| `{{name}}` | project / repo name |
| `{{file}}` `{{file.name}}` `{{file.stem}}` | the matched file (only with `per_file: true`) |
| `{{env:VAR}}` | an environment variable |
| `{{<program-key>}}` | a resolved program path |

---

## Recipes

All of these go in `rules.yaml`.

**Pin an editor that auto-detection misses**

```yaml
programs:
  anypoint:
    windows:
      - "D:/tools/AnypointStudio/AnypointStudio.exe"
```

**Add a project rule** — `Makefile` → `make` / `make test` in a terminal

```yaml
rules:
  - id: make
    match: Makefile
    requires: [make]
    actions:
      - { name: "make", run: "make", terminal: true }
      - { name: "make test", run: "make test", terminal: true }
```

**Add a universal launcher** — open every repo in Helix in a terminal

```yaml
programs:
  helix: { any: [hx] }
universal:
  add:
    - { id: helix, name: "Edit in Helix", run: "hx {{path}}", terminal: true, needs: [helix] }
```

**Turn off a built-in rule**

```yaml
rules_disable: [docker-image]
```

**Change what Enter does on a repo in the main list**

```yaml
universal:
  default: vscode
```

**Start markers from scratch** (only git repos show up)

```yaml
markers_replace: true
markers:
  - .git
```

---

## Applying changes

- **Settings ▸ Rules ▸ Reload config** re-reads `config.yaml` + `rules.yaml`,
  clears the program-lookup cache, and rescans.
- The Settings "Active configuration" panel shows the merged result — which
  programs resolved, which rules are live vs. unmet vs. disabled.
- A malformed `rules.yaml` surfaces as an error on Reload (and blocks the merge)
  rather than being silently ignored.
