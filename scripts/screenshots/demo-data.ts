// The fixed world the screenshots are taken in: the demo repo set from
// scripts/clone-showcase.mjs, the actions the real backend produces for
// them, a handful of installed apps, and recorded shell output. Shapes match
// src/lib/types.ts — this file is the backend, as far as the frontend knows.

import * as si from "simple-icons";
import type {
  Action,
  AppEntry,
  Branch,
  BranchList,
  NewRepoOptions,
  RemovalAssessment,
  Repo,
  RepoStatus,
  VcsDetails,
} from "../../src/lib/types";

export const ROOT = "/home/dev/code";

const repo = (name: string, sentinels: string[], vcs: string | null = "Git"): Repo => ({
  name,
  path: `${ROOT}/${name}`,
  vcs,
  sentinels,
  lastSeen: 1_792_000_000,
});

export const repos: Repo[] = [
  repo("alacritty", ["Cargo.toml"]),
  repo("django", ["package.json", "pyproject.toml"]),
  repo("helix", ["Cargo.toml", "flake.nix", "shell.nix"]),
  repo("httpie", ["setup.cfg", "setup.py"]),
  repo("hugo", ["Dockerfile", "go.mod"]),
  repo("lazygit", ["Dockerfile", "flake.nix", "go.mod"]),
  repo("neovim", ["CMakeLists.txt"]),
  repo("prettier", ["package.json"]),
  repo("ripgrep", ["Cargo.toml"]),
  repo("starship", ["Cargo.toml"]),
  // Folders without version control, for the footer's folder count.
  repo("notes", [], null),
  repo("scratch", [], null),
];

const action = (a: Partial<Action> & Pick<Action, "id" | "label">): Action => ({
  hint: "",
  group: "General",
  cluster: "",
  usageScore: 0,
  clientSide: false,
  ...a,
});

/** The always-available actions, as `default_config.yaml` resolves them on a
 *  Linux box with VS Code, Zed, Claude Code and lazygit installed. */
export function universal(path: string): Action[] {
  const vcs = repos.find((r) => r.path === path)?.vcs;
  const out: Action[] = [
    action({ id: "terminal", label: "Open in terminal", icon: "terminal", hotkey: "Ctrl+T", hint: "konsole" }),
    action({ id: "filemanager", label: "Show in file manager", icon: "folder", hotkey: "Ctrl+E", hint: `dolphin ${path}` }),
    action({ id: "copy-path", label: "Copy path", icon: "copy", hotkey: "Ctrl+C", clientSide: true }),
    action({ id: "run-command", label: "Open shell", icon: "run", hotkey: "Ctrl+R", prompt: true }),
    action({ id: "claude-code", label: "Launch Claude Code", icon: "claude", cluster: "ai-cli", hint: "claude" }),
    action({ id: "vscode", label: "Open in VS Code", icon: "vscode", cluster: "ide", hint: `code ${path}` }),
    action({ id: "zed", label: "Open in Zed", icon: "zed", cluster: "ide", hint: `zed ${path}` }),
    action({ id: "lazygit", label: "lazygit", icon: "git", cluster: "git", hint: "lazygit" }),
    action({ id: "remove-folder", label: "Move to Trash…", icon: "trash", cluster: "remove", remove: true }),
  ];
  if (!vcs) {
    out.splice(4, 0, action({ id: "set-up-vcs", label: "Set up version control…", icon: "branch", provision: true }));
  }
  return out;
}

const detectedFor: Record<string, [string, string][]> = {
  // [label, icon] — the providers' own wording (`<manager> <script>`, …).
  prettier: [
    ["yarn build", "yarn"],
    ["yarn fix", "yarn"],
    ["yarn lint", "yarn"],
    ["yarn test", "yarn"],
    ["yarn test:dev-package", "yarn"],
    ["yarn test:production", "yarn"],
  ],
  hugo: [
    ["go build ./...", "go"],
    ["go test ./...", "go"],
    ["go run .", "go"],
    ["docker build", "docker"],
  ],
  lazygit: [
    ["go build ./...", "go"],
    ["go test ./...", "go"],
    ["go run .", "go"],
    ["docker build", "docker"],
    ["nix develop", "nix"],
  ],
  django: [
    ["npm run test", "npm"],
    ["uv sync", "python"],
    ["uv run pytest", "python"],
  ],
};
for (const name of ["alacritty", "helix", "ripgrep", "starship"]) {
  detectedFor[name] = [
    ["cargo build", "rust"],
    ["cargo test", "rust"],
    ["cargo run", "rust"],
    ["cargo clippy", "rust"],
  ];
}

export function detected(path: string): Action[] {
  const name = path.split("/").pop() ?? "";
  return (detectedFor[name] ?? []).map(([label, icon], i) =>
    action({ id: `det:${name}:${i}`, label, icon, group: "Detected", hint: label }),
  );
}

// --- installed apps (the `>` scope) ------------------------------------------

type Icon = { svg: string; hex: string };
const iconUrl = (i: Icon) =>
  `data:image/svg+xml;base64,${btoa(i.svg.replace("<svg ", `<svg fill="#${i.hex}" `))}`;

const app = (name: string, exec: string, icon: Icon, uses: number): AppEntry => ({
  name,
  exec: `/usr/share/applications/${exec}.desktop`,
  kind: "desktop",
  args: [exec],
  icon: iconUrl(icon),
  source: "desktop",
  uses,
});

export const apps: AppEntry[] = [
  app("Firefox", "firefox", si.siFirefoxbrowser, 42),
  app("Alacritty", "alacritty", si.siAlacritty, 30),
  app("Obsidian", "obsidian", si.siObsidian, 21),
  app("Thunderbird", "thunderbird", si.siThunderbird, 12),
  app("GIMP", "gimp", si.siGimp, 9),
  app("Inkscape", "inkscape", si.siInkscape, 8),
  app("Blender", "blender", si.siBlender, 6),
  app("KiCad", "kicad", si.siKicad, 4),
  app("Steam", "steam", si.siSteam, 3),
  app("Signal", "signal-desktop", si.siSignal, 2),
  app("Kdenlive", "kdenlive", si.siKdenlive, 1),
  app("LibreOffice Writer", "libreoffice-writer", si.siLibreofficewriter, 1),
];

// --- New repository / Move to Trash ------------------------------------------

export const newRepoOptions: NewRepoOptions = {
  roots: [ROOT],
  tools: [
    {
      id: "git",
      label: "Git",
      fields: [{ id: "branch", label: "Initial branch", default: "main" }],
      run: [["git", "init", "--initial-branch={{branch}}"]],
    },
    { id: "jj", label: "Jujutsu", fields: [], run: [["jj", "git", "init"]] },
    { id: "hg", label: "Mercurial", fields: [], run: [["hg", "init"]] },
  ],
};

export function assessment(path: string): RemovalAssessment {
  const name = path.split("/").pop() ?? path;
  return {
    name,
    path,
    vcs: "Git",
    risks: [
      "2 files with uncommitted changes",
      "3 commits on feature/split-view that aren't on any remote",
      "1 stash",
    ],
    unknown: [],
    needsTyped: true,
  };
}

// --- working-copy status and branches -----------------------------------------

/** Ages are counted from the page's clock, so they read the same every run. */
const NOW = Math.floor(Date.now() / 1000);

const status = (over: Partial<RepoStatus>): RepoStatus => ({
  vcs: "Git",
  branch: "master",
  beyond: 0,
  unborn: false,
  upstream: `origin/${over.branch ?? "master"}`,
  upstreamGone: false,
  ahead: 0,
  behind: 0,
  changed: 0,
  untracked: 0,
  conflicted: 0,
  conflictedChanges: 0,
  divergent: 0,
  stashes: 0,
  unpushed: 0,
  noRemote: false,
  fetchedAt: NOW - 2 * 3600,
  checkedAt: NOW,
  unknown: [],
  ...over,
});

/** A spread of states: clean, behind, stashed, unpushed work on an
 *  unpublished branch (the one the Trash shot assesses), a rebase stopped on
 *  a conflict, and work in progress on a tracked branch. */
export const statuses: Record<string, RepoStatus> = {
  [`${ROOT}/alacritty`]: status({}),
  [`${ROOT}/django`]: status({ branch: "main", behind: 6 }),
  [`${ROOT}/helix`]: status({}),
  [`${ROOT}/httpie`]: status({ stashes: 2 }),
  [`${ROOT}/hugo`]: status({}),
  [`${ROOT}/lazygit`]: status({
    branch: "feature/split-view",
    upstream: null,
    changed: 2,
    unpushed: 3,
    stashes: 1,
  }),
  [`${ROOT}/neovim`]: status({
    branch: "fix/lsp-hover",
    upstream: "origin/fix/lsp-hover",
    detached: "5f3a9c1",
    operation: "rebase",
    conflicted: 1,
    changed: 2,
  }),
  [`${ROOT}/prettier`]: status({ branch: "main", ahead: 2, unpushed: 2, changed: 1, untracked: 1 }),
  [`${ROOT}/ripgrep`]: status({}),
  [`${ROOT}/starship`]: status({ branch: "feat/jj-module", upstream: null, unpushed: 4 }),
};

const OWNER: Record<string, string> = {
  alacritty: "alacritty",
  django: "django",
  helix: "helix-editor",
  httpie: "httpie",
  hugo: "gohugoio",
  lazygit: "jesseduffield",
  neovim: "neovim",
  prettier: "prettier",
  ripgrep: "BurntSushi",
  starship: "starship",
};

export function gitDetails(path: string): VcsDetails {
  const name = path.split("/").pop() ?? "";
  const s = statuses[path];
  const base = `https://github.com/${OWNER[name] ?? "dev"}/${name}`;
  const def = s?.branch === "main" ? "main" : "master";
  const links: VcsDetails["links"] = [{ kind: "repo", label: "Open on GitHub", url: base }];
  const pushed = s?.upstream && !s.upstreamGone ? s.upstream.replace(/^origin\//, "") : null;
  if (pushed) {
    links.push({ kind: "branch", label: `Open ${pushed} on GitHub`, url: `${base}/tree/${pushed}` });
    if (pushed !== def) {
      links.push({ kind: "pr", label: "Create pull request on GitHub", url: `${base}/compare/${def}...${pushed}?expand=1` });
    }
  }
  return { remotes: ["origin"], defaultBranch: def, links };
}

const branch = (name: string, daysAgo: number, subject: string, over: Partial<Branch> = {}): Branch => ({
  name,
  kind: over.remote ? "remote" : "branch",
  remote: null,
  current: false,
  upstream: `origin/${name}`,
  ahead: 0,
  behind: 0,
  gone: false,
  worktree: null,
  date: NOW - daysAgo * 86400,
  subject,
  ...over,
});

export function branches(path: string): BranchList {
  const s = statuses[path];
  const current = s?.branch ?? "master";
  const list: Branch[] = [
    branch(current, 0.2, "Keep the split ratio when the terminal resizes", { current: true, upstream: s?.upstream ?? null, ahead: s?.ahead ?? 0, behind: s?.behind ?? 0 }),
    branch("master", 1, "Bump go-git to v5.16", { behind: 12 }),
    branch("fix/staging-hunks", 3, "Stage a hunk from the diff view", {
      worktree: `${ROOT}/lazygit-fix-staging-hunks`,
    }),
    branch("perf/commit-graph", 9, "Draw the commit graph incrementally", { ahead: 2 }),
    branch("old/rebase-ui", 64, "WIP interactive rebase todo editor", { upstream: "origin/old/rebase-ui", gone: true }),
    branch("origin/feature/custom-pagers", 5, "Let each repo pick its own pager", { remote: "origin", upstream: null }),
    branch("origin/release/v0.52", 21, "Release notes for v0.52", { remote: "origin", upstream: null }),
  ].filter((b, i) => i === 0 || b.name !== current);
  return {
    branches: list,
    worktrees: [
      { path, branch: current, head: "a1b2c3d", current: true, prunable: false },
      { path: `${ROOT}/lazygit-fix-staging-hunks`, branch: "fix/staging-hunks", head: "9e8d7c6", current: false, prunable: false },
    ],
    remotes: ["origin"],
  };
}

// --- shell sessions ----------------------------------------------------------

const ESC = "\x1b[";
const c = (code: string, s: string) => `${ESC}${code}m${s}${ESC}0m`;
const prompt = (dir: string, branch: string) =>
  `${c("1;34", `~/code/${dir}`)} ${c("35", branch)} ${c("32", "❯")} `;
const crlf = (s: string) => s.replace(/\n/g, "\r\n");

/** What a shell in each repo "prints" when it opens: a prompt, a command or
 *  two already run, and a fresh prompt. */
export function transcript(cwd: string): string {
  const name = cwd.split("/").pop() ?? "";
  if (name === "hugo") {
    return crlf(
      [
        prompt("hugo", "master") + "git log --oneline --graph -6",
        `* ${c("33", "4f1c2a9")} ${c("33", "(")}${c("1;36", "HEAD")}${c("33", " -> ")}${c("1;32", "master")}${c("33", ", ")}${c("1;31", "origin/master")}${c("33", ")")} tpl: Add strings.Diff`,
        `* ${c("33", "b83e0d7")} markup/goldmark: Fix table alignment with inline code`,
        `*   ${c("33", "91aa6c2")} Merge pull request #13402 from bep/feat/partial-cache`,
        `${c("31", "|")}${c("32", "\\")}  `,
        `${c("31", "|")} * ${c("33", "e27b9f0")} tpl/partials: Cache keyed on the full context`,
        `${c("31", "|")}${c("31", "/")}  `,
        `* ${c("33", "0c5d8e1")} deps: Upgrade github.com/evanw/esbuild v0.25.9 => v0.25.10`,
        "",
        prompt("hugo", "master") + "hugo server -D",
        `Watching for changes in ${c("36", "/home/dev/code/hugo/docs/{archetypes,assets,content,layouts,static}")}`,
        "Start building sites … ",
        `hugo v0.151.0-${c("2", "4f1c2a9")}+extended linux/amd64 BuildDate=2026-10-02T09:12:44Z`,
        "",
        `                   │ ${c("1", "EN")}`,
        "───────────────────┼─────",
        "  Pages            │ 612",
        "  Paginator pages  │  28",
        "  Static files     │ 189",
        "  Processed images │  74",
        "",
        `Built in ${c("32", "1342 ms")}`,
        `Web Server is available at ${c("4;36", "http://localhost:1313/")} (bind address 127.0.0.1)`,
        `Press ${c("1", "Ctrl+C")} to stop`,
      ].join("\n"),
    );
  }
  if (name === "prettier") {
    return crlf(
      [
        prompt("prettier", "main") + "yarn test tests/format/js/arrows",
        `${c("1;30;42", " PASS ")} ${c("2", "tests/format/js/arrows/")}${c("1", "format.test.js")}`,
        `  ✓ arrow-call.js ${c("2", "(18 ms)")}`,
        `  ✓ currying.js ${c("2", "(11 ms)")}`,
        `  ✓ parens.js ${c("2", "(9 ms)")}`,
        "",
        `${c("1", "Tests:")}       ${c("1;32", "126 passed")}, 126 total`,
        `${c("1", "Time:")}        3.41 s`,
        "",
        prompt("prettier", "main"),
      ].join("\n"),
    );
  }
  return crlf(prompt(name, "main"));
}
