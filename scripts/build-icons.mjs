// Regenerates src/lib/icons.ts from the `simple-icons` dev dependency.
//
//   node scripts/build-icons.mjs
//
// Brand glyphs come from Simple Icons (https://simpleicons.org, the icon set is
// CC0). Trademarks belong to their owners; dev-prompt shows them only to
// identify the tool an action launches. Icons the Simple Icons project has
// pruned for trademark reasons (VS Code, Visual Studio, Windows Terminal, …)
// fall back to a neutral generic glyph — see GENERIC below.

import { writeFileSync } from "node:fs";
import * as si from "simple-icons";

// dev-prompt icon key -> simple-icons slug
const BRAND = {
  rider: "rider",
  intellij: "intellijidea",
  webstorm: "webstorm",
  pycharm: "pycharm",
  goland: "goland",
  clion: "clion",
  jetbrains: "jetbrains",
  cursor: "cursor",
  zed: "zedindustries",
  sublime: "sublimetext",
  neovim: "neovim",
  helix: "helix",
  claude: "claude",
  eclipse: "eclipseide",
  git: "git",
  github: "github",
  gitlab: "gitlab",
  gitkraken: "gitkraken",
  tmux: "tmux",
  homebrew: "homebrew",
  docker: "docker",
  podman: "podman",
  kubernetes: "kubernetes",
  rust: "rust",
  go: "go",
  python: "python",
  node: "nodedotjs",
  npm: "npm",
  pnpm: "pnpm",
  yarn: "yarn",
  bun: "bun",
  deno: "deno",
  dotnet: "dotnet",
  java: "openjdk",
  gradle: "gradle",
  kotlin: "kotlin",
  scala: "scala",
  swift: "swift",
  dart: "dart",
  flutter: "flutter",
  c: "c",
  cpp: "cplusplus",
  php: "php",
  composer: "composer",
  laravel: "laravel",
  ruby: "ruby",
  rubygems: "rubygems",
  rails: "rubyonrails",
  elixir: "elixir",
  erlang: "erlang",
  nix: "nixos",
  bash: "gnubash",
  postgres: "postgresql",
  mysql: "mysql",
  redis: "redis",
  mongodb: "mongodb",
  sqlite: "sqlite",
  firefox: "firefoxbrowser",
  chrome: "googlechrome",
};

// Logos Simple Icons has pruned for trademark reasons. Monochrome `-plain`
// paths from devicon (https://devicon.dev, MIT); `fill` stripped so they take
// the tint. `raw` is inner SVG markup on the given `vb` viewBox.
const MANUAL = {
  vscode: {
    hex: "#3C99D4",
    vb: "0 0 128 128",
    raw: '<path fill-rule="evenodd" clip-rule="evenodd" d="M90.767 127.126a7.968 7.968 0 0 0 6.35-.244l26.353-12.681a8 8 0 0 0 4.53-7.209V21.009a8 8 0 0 0-4.53-7.21L97.117 1.12a7.97 7.97 0 0 0-9.093 1.548l-50.45 46.026L15.6 32.013a5.328 5.328 0 0 0-6.807.302l-7.048 6.411a5.335 5.335 0 0 0-.006 7.888L20.796 64 1.74 81.387a5.336 5.336 0 0 0 .006 7.887l7.048 6.411a5.327 5.327 0 0 0 6.807.303l21.974-16.68 50.45 46.025a7.96 7.96 0 0 0 2.743 1.793Zm5.252-92.183L57.74 64l38.28 29.058V34.943Z"/>',
  },
  vs: {
    hex: "#8A55C2",
    vb: "0 0 128 128",
    raw: '<path d="M94.145.348a8 8 0 0 0-.563.072 8 8 0 0 0-.928.027 8 8 0 0 0-.982.215 8 8 0 0 0-.553.072 8 8 0 0 0-.822.387 8 8 0 0 0-.486.229A8 8 0 0 0 88 2.668L45.486 49.674l-24.82-20.34-2.172-1.865a5.333 5.333 0 0 0-.826-.463 5.333 5.333 0 0 0-.535-.3 5.333 5.333 0 0 0-.604-.227 5.333 5.333 0 0 0-.644-.15 5.333 5.333 0 0 0-.504-.017 5.333 5.333 0 0 0-.51-.074 5.333 5.333 0 0 0-.266.06 5.333 5.333 0 0 0-.214-.003 5.333 5.333 0 0 0-.227.06 5.333 5.333 0 0 0-.484.006 3.4 3.4 0 0 0-.707.24l-9.694 4.067a5.333 5.333 0 0 0-1.466.95 5.333 5.333 0 0 0-.215.177 5.333 5.333 0 0 0-.23.312 5.333 5.333 0 0 0-.64.88 5.333 5.333 0 0 0-.267.476 5.333 5.333 0 0 0-.09.275 5.333 5.333 0 0 0-.232.908 5.333 5.333 0 0 0-.139.516v57.68a5.333 5.333 0 0 0 .139.512 5.333 5.333 0 0 0 .232.914 5.333 5.333 0 0 0 .09.271 5.333 5.333 0 0 0 .268.477 5.333 5.333 0 0 0 .638.879 5.333 5.333 0 0 0 .23.312 5.333 5.333 0 0 0 .216.178 5.333 5.333 0 0 0 1.466.949l9.694 4.066a5.333 5.333 0 0 0 1.716.37 5.333 5.333 0 0 0 .51-.01 5.333 5.333 0 0 0 1.272-.198 5.333 5.333 0 0 0 .51-.154 5.333 5.333 0 0 0 1.513-.873l2.172-1.867 24.82-20.34L88 125.334a8 8 0 0 0 1.902 1.387 8 8 0 0 0 .604.23 8 8 0 0 0 .006.002 8 8 0 0 0 1.342.516 8 8 0 0 0 .242.013 8 8 0 0 0 1.558.088 8 8 0 0 0 .99.014 8 8 0 0 0 2.45-.703l26.373-12.68a8 8 0 0 0 .572-.306 8 8 0 0 0 .17-.106 8 8 0 0 0 .383-.25 8 8 0 0 0 .181-.135 8 8 0 0 0 .33-.258 8 8 0 0 0 .204-.173 8 8 0 0 0 .277-.26 8 8 0 0 0 .19-.19 8 8 0 0 0 .283-.314 8 8 0 0 0 .144-.166 8 8 0 0 0 .266-.352 8 8 0 0 0 .142-.193 8 8 0 0 0 .33-.53 8 8 0 0 0 .09-.17 8 8 0 0 0 .2-.386 8 8 0 0 0 .113-.256 8 8 0 0 0 .152-.379 8 8 0 0 0 .076-.215 8 8 0 0 0 .116-.367 8 8 0 0 0 .082-.304 8 8 0 0 0 .072-.342 8 8 0 0 0 .053-.274 8 8 0 0 0 .056-.431 8 8 0 0 0 .022-.18 8 8 0 0 0 .002-.023 8 8 0 0 0 .027-.653V21.027a8 8 0 0 0 0-.011 8 8 0 0 0-.027-.655 8 8 0 0 0-.018-.158 8 8 0 0 0-.066-.508 8 8 0 0 0-.034-.17 8 8 0 0 0-.095-.443 8 8 0 0 0-.073-.268 8 8 0 0 0-.103-.33 8 8 0 0 0-.102-.295 8 8 0 0 0-.142-.345 8 8 0 0 0-.1-.233 8 8 0 0 0-.209-.404 8 8 0 0 0-.103-.195 8 8 0 0 0-.332-.528 8 8 0 0 0-.106-.14 8 8 0 0 0-.277-.37 8 8 0 0 0-.188-.216 8 8 0 0 0-.246-.274 8 8 0 0 0-.193-.193 8 8 0 0 0-.28-.262 8 8 0 0 0-.202-.174 8 8 0 0 0-.33-.257 8 8 0 0 0-.182-.135 8 8 0 0 0-.383-.25 8 8 0 0 0-.17-.106 8 8 0 0 0-.572-.306L97.094 1.12A8 8 0 0 0 94.75.416a8 8 0 0 0-.342.002 8 8 0 0 0-.263-.07zM96 36.908v54.186L62.947 64.002 96 36.908zm-80 8.82 16.527 18.274L16 82.275V45.73z"/>',
  },
};

// Neutral filled glyphs (24x24) for things without — or not allowed — a logo.
const GENERIC = {
  app: { d: "M7 3h10a4 4 0 0 1 4 4v10a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4V7a4 4 0 0 1 4-4z" },
  run: { d: "M8 5v14l11-7z" },
  terminal: {
    d: "M3 4h18a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zm3.7 4.3L10 11.6l-3.3 3.3 1.4 1.4L12.8 12 8.1 7.3 6.7 8.3zM13 15h5v2h-5z",
  },
  folder: {
    d: "M4 4h6l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
  },
  copy: {
    d: "M9 3h9a2 2 0 0 1 2 2v9h-2V5H9V3zM5 7h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2z",
  },
  code: {
    d: "M9.4 16.6 4.8 12l4.6-4.6L8 6l-6 6 6 6 1.4-1.4zm5.2 0L19.2 12l-4.6-4.6L16 6l6 6-6 6-1.4-1.4z",
  },
};

const pascal = (s) => "si" + s.charAt(0).toUpperCase() + s.slice(1);

const brand = {};
const missing = [];
for (const [key, slug] of Object.entries(BRAND)) {
  const ic = si[pascal(slug)];
  if (!ic) {
    missing.push(`${key} -> ${slug}`);
    continue;
  }
  brand[key] = { d: ic.path, hex: `#${ic.hex}` };
}
if (missing.length) console.warn("skipped (not in simple-icons):", missing.join(", "));

const merged = { ...GENERIC, ...brand, ...MANUAL };
const body = `// AUTO-GENERATED by scripts/build-icons.mjs — do not edit by hand.
// Brand glyphs: Simple Icons (https://simpleicons.org, CC0) + a few devicon
// (MIT) monochrome paths. Trademarks belong to their owners; shown here only to
// identify the tool an action launches.

export interface Glyph {
  /** Single path on a 24x24 viewBox (Simple Icons). */
  d?: string;
  /** Inner SVG markup, for glyphs that aren't one 24x24 path. */
  raw?: string;
  /** viewBox override for \`raw\` glyphs (default "0 0 24 24"). */
  vb?: string;
  /** Brand color, when the icon has one. */
  hex?: string;
}

export const icons: Record<string, Glyph> = ${JSON.stringify(merged, null, 2)};

export const iconKeys: string[] = Object.keys(icons).sort();
`;

writeFileSync(new URL("../src/lib/icons.ts", import.meta.url), body);
console.log(`wrote src/lib/icons.ts — ${Object.keys(merged).length} icons`);
