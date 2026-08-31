// CalVer generator: YYYY.(MM*100+DD).BUILD
//
// major = calendar year               e.g. 2026
// minor = month*100 + day             e.g. Aug 31 -> 831, Jan 5 -> 105
// build = 1 for the day's first release, then +1 per additional same-day build
//
// The build segment is derived from existing git tags (v<major>.<minor>.*), so
// nothing needs to be committed or hand-incremented. Writes the computed version
// into package.json, src-tauri/tauri.conf.json and src-tauri/Cargo.toml, then
// prints just the version string to stdout for the release workflow to consume.
// Diagnostics go to stderr so `VERSION=$(node scripts/version.mjs)` stays clean.

import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const now = new Date();
const major = now.getUTCFullYear();
const minor = (now.getUTCMonth() + 1) * 100 + now.getUTCDate();

let build = 1;
try {
  const tags = execSync(`git tag --list "v${major}.${minor}.*"`, {
    encoding: "utf8",
    cwd: root,
  })
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
  const builds = tags
    .map((t) => t.match(/^v\d+\.\d+\.(\d+)$/))
    .filter(Boolean)
    .map((m) => Number(m[1]));
  if (builds.length) build = Math.max(...builds) + 1;
} catch (e) {
  process.stderr.write(`warning: could not read git tags (${e.message})\n`);
}

const version = `${major}.${minor}.${build}`;
process.stderr.write(`computed version: ${version}\n`);

// --- package.json ---------------------------------------------------------
const pkgPath = join(root, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

// --- src-tauri/tauri.conf.json -------------------------------------------
const confPath = join(root, "src-tauri", "tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));
conf.version = version;
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");

// --- src-tauri/Cargo.toml ----------------------------------------------
// Replace the first `version = "..."` line, which is the [package] version
// since [package] is the first table in the file.
const cargoPath = join(root, "src-tauri", "Cargo.toml");
const cargo = readFileSync(cargoPath, "utf8").replace(
  /^version = ".*"$/m,
  `version = "${version}"`,
);
writeFileSync(cargoPath, cargo);

process.stdout.write(version);
