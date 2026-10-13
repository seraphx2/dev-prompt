// Fails when a Tauri npm package and its Rust crate are on different
// major.minor versions — the same check `tauri build` runs, so a mismatch
// shows up on the PR instead of in the release build. npm and Cargo are
// updated separately (Dependabot groups them by ecosystem), which is how the
// two sides drift apart.
//
// Pairs: @tauri-apps/api ↔ tauri, @tauri-apps/plugin-<x> ↔ tauri-plugin-<x>,
// for every one the app depends on directly. Reads the lockfiles only, so it
// needs no install or build.
//
//   node scripts/check-tauri-versions.mjs [package-lock.json] [Cargo.lock]

import { readFileSync } from "node:fs";

const [npmLock = "package-lock.json", cargoLock = "src-tauri/Cargo.lock"] = process.argv.slice(2);

const pkgs = JSON.parse(readFileSync(npmLock, "utf8")).packages;
const root = pkgs[""];
const direct = Object.keys({ ...root.dependencies, ...root.devDependencies });

/** Every version of each crate in Cargo.lock (a crate can appear twice). */
const crates = new Map();
for (const block of readFileSync(cargoLock, "utf8").split("[[package]]")) {
  const name = block.match(/^name = "([^"]+)"/m)?.[1];
  const version = block.match(/^version = "([^"]+)"/m)?.[1];
  if (name && version) crates.set(name, [...(crates.get(name) ?? []), version]);
}

const minor = (v) => v.split(".").slice(0, 2).join(".");

const pairs = direct
  .map((name) => {
    if (name === "@tauri-apps/api") return [name, "tauri"];
    const plugin = name.match(/^@tauri-apps\/plugin-(.+)$/)?.[1];
    return plugin ? [name, `tauri-plugin-${plugin}`] : null;
  })
  .filter(Boolean);

let bad = 0;
for (const [npm, crate] of pairs) {
  const npmVersion = pkgs[`node_modules/${npm}`]?.version;
  const crateVersions = crates.get(crate) ?? [];
  let problem = null;
  if (!npmVersion) problem = "isn't in package-lock.json";
  else if (!crateVersions.length) problem = `has no ${crate} crate in Cargo.lock`;
  else if (!crateVersions.some((v) => minor(v) === minor(npmVersion))) problem = "differs";
  const line = `${crate} ${crateVersions.join("/") || "—"} : ${npm} ${npmVersion ?? "—"}`;
  if (problem) {
    bad++;
    console.error(`MISMATCH ${line}${problem === "differs" ? "" : ` (${npm} ${problem})`}`);
  } else {
    console.log(`ok       ${line}`);
  }
}

if (bad) {
  console.error(
    `\n${bad} Tauri ${bad === 1 ? "pair is" : "pairs are"} on different major.minor versions, ` +
      "and `tauri build` will refuse to build. Update the npm package or the crate " +
      "(`cargo update -p <crate>` in src-tauri) so they match.",
  );
  process.exit(1);
}
console.log(`\n${pairs.length} Tauri pairs match.`);
