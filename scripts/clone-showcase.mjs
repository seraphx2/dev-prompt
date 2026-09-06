// Clone a curated set of well-known public repos into a folder for demo /
// screenshot purposes (see docs/img/README.md — the Flathub screenshot).
//
// The set is deliberately a believable working-developer checkout: popular CLI
// tools across Rust / Go / Python / Node / C, one project per repo so the
// overlay list stays clean (no monorepo packages/* nesting), and each repo
// triggers a different default rule so the action menu shows real per-project
// actions (cargo build/test, npm scripts, go test, …).
//
// Usage:
//   node scripts/clone-showcase.mjs [targetDir] [--depth N] [--dry-run]
//
//   targetDir   where to clone (default: C:\repos on Windows, ~/repos elsewhere,
//               or $SHOWCASE_DIR if set)
//   --depth N   git clone depth (default: 1; use 0 for a full clone)
//   --prune     also delete subfolders of targetDir that are git clones but not
//               in the set below (reconciles an earlier, longer run)
//   --dry-run   print what would happen, change nothing
//
// Already-populated target subdirectories are left untouched, so re-running is
// safe and only fills in what's missing.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// "owner/repo" on github.com  ->  cloned as <targetDir>/<repo>
// "owner/repo:dir"            ->  cloned as <targetDir>/<dir>
//
// ~10 repos: only 8-9 rows are visible in the overlay at once, so a longer list
// buys nothing. Names chosen to read as real project names in the list (no terse
// `fd` / `bat` / `gh` abbreviations), one project per repo, spread across
// ecosystems so the action menu shows different per-project rules.
const REPOS = [
  // Rust — cargo rule (build / test / run / bench)
  "BurntSushi/ripgrep",
  "alacritty/alacritty",
  "starship/starship",
  "helix-editor/helix",
  // Go — go rule (build / test / vet / run)
  "jesseduffield/lazygit",
  "gohugoio/hugo",
  // Python — python rule (run module / test / editable install)
  "httpie/cli:httpie",
  "django/django",
  // Node — npm-scripts provider (one action per package.json script)
  "prettier/prettier",
  // C — cmake rule (configure / build)
  "neovim/neovim",
];

const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const prune = args.includes("--prune");
const depthIdx = args.indexOf("--depth");
const depth = depthIdx !== -1 ? Number(args[depthIdx + 1]) : 1;
if (!Number.isInteger(depth) || depth < 0) {
  console.error(`invalid --depth: ${args[depthIdx + 1]}`);
  process.exit(2);
}
const positional = args.filter(
  (a, i) => !a.startsWith("--") && !(depthIdx !== -1 && i === depthIdx + 1),
);

const defaultDir =
  process.env.SHOWCASE_DIR ||
  (process.platform === "win32" ? "C:\\repos" : join(homedir(), "repos"));
const targetDir = positional[0] || defaultDir;

// fail early if git is missing
try {
  execFileSync("git", ["--version"], { stdio: "ignore" });
} catch {
  console.error("git not found on PATH");
  process.exit(1);
}

if (!existsSync(targetDir)) {
  console.log(`creating ${targetDir}`);
  if (!dryRun) mkdirSync(targetDir, { recursive: true });
}

const nonEmpty = (dir) => existsSync(dir) && readdirSync(dir).length > 0;

let cloned = 0;
let skipped = 0;
let failed = 0;
let pruned = 0;

const seen = new Set();
for (const entry of REPOS) {
  const [slug, override] = entry.split(":");
  const name = override || slug.split("/")[1];
  if (seen.has(name)) {
    console.error(`duplicate destination "${name}" in REPOS — fix the list`);
    process.exit(2);
  }
  seen.add(name);
  const dest = join(targetDir, name);
  if (nonEmpty(dest)) {
    console.log(`skip   ${name} (already present)`);
    skipped++;
    continue;
  }
  const url = `https://github.com/${slug}.git`;
  const gitArgs = ["clone", "--quiet"];
  if (depth > 0) gitArgs.push("--depth", String(depth));
  gitArgs.push(url, dest);

  if (dryRun) {
    console.log(`clone  ${name}  <-  git ${gitArgs.join(" ")}`);
    cloned++;
    continue;
  }

  process.stdout.write(`clone  ${name} ... `);
  try {
    execFileSync("git", gitArgs, { stdio: ["ignore", "ignore", "inherit"] });
    console.log("ok");
    cloned++;
  } catch {
    console.log("FAILED");
    failed++;
  }
}

if (prune && existsSync(targetDir)) {
  for (const child of readdirSync(targetDir)) {
    if (seen.has(child)) continue;
    const dir = join(targetDir, child);
    if (!statSync(dir).isDirectory() || !existsSync(join(dir, ".git"))) continue;
    console.log(`prune  ${child}`);
    if (!dryRun) rmSync(dir, { recursive: true, force: true });
    pruned++;
  }
}

console.log(
  `\n${dryRun ? "[dry run] " : ""}${cloned} cloned, ${skipped} skipped, ${pruned} pruned, ${failed} failed  ->  ${targetDir}`,
);
if (!dryRun && failed === 0) {
  console.log(
    `\nNext: point dev-prompt's scan root at ${targetDir} (Settings > roots),\n` +
      `Reload config, press the hotkey, type a 2-char query, capture docs/img/overlay.png.`,
  );
}
process.exit(failed > 0 ? 1 : 0);
