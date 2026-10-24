// How a repo row shows what a project is: the project files the scan found
// (`Repo.sentinels`) as one small brand icon per ecosystem — the file names
// in its tooltip — and its version control as the short command name, left
// out for Git (nearly every repo is). A file no ecosystem claims (a marker
// added in rules.yaml) stays a text chip, so nothing found goes unshown.

/** An ecosystem the row shows as one icon. */
export interface Ecosystem {
  /** Key into `icons.ts`. */
  icon: string;
  name: string;
  /** The files found that put the repo in it. */
  files: string[];
}

/** Project file (as the scan labels it: `*.sln` → `sln`) → icon and name. */
const BY_FILE: [string[], string, string][] = [
  [["package.json", "pnpm-workspace.yaml"], "node", "Node.js"],
  [["deno.json", "deno.jsonc"], "deno", "Deno"],
  [["Cargo.toml"], "rust", "Rust"],
  [["go.mod", "go.work"], "go", "Go"],
  [["pyproject.toml", "requirements.txt", "setup.py", "setup.cfg", "Pipfile", "manage.py"], "python", "Python"],
  [["sln", "slnx", "csproj", "fsproj", "vbproj"], "dotnet", ".NET"],
  [["Dockerfile", "docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"], "docker", "Docker"],
  [["pom.xml"], "java", "Java (Maven)"],
  [["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"], "gradle", "Gradle"],
  [["CMakeLists.txt"], "cpp", "CMake"],
  [["Gemfile"], "ruby", "Ruby"],
  [["composer.json"], "php", "PHP"],
  [["mix.exs"], "elixir", "Elixir"],
  [["pubspec.yaml"], "dart", "Dart / Flutter"],
  [["flake.nix", "shell.nix"], "nix", "Nix"],
  [[".project", "mule-artifact.json"], "eclipse", "Eclipse"],
  [["code-workspace"], "vscode", "VS Code workspace"],
  // A second version control in the same folder — jj colocated with Git.
  [[".git"], "git", "Colocated with Git"],
];

/** The ecosystems `sentinels` add up to (in the order first found), and the
 *  files none of them claims. */
export function ecosystems(sentinels: string[]): { icons: Ecosystem[]; others: string[] } {
  const icons: Ecosystem[] = [];
  const others: string[] = [];
  for (const file of sentinels) {
    const hit = BY_FILE.find(([files]) => files.some((f) => f.toLowerCase() === file.toLowerCase()));
    if (!hit) {
      others.push(file);
      continue;
    }
    const [, icon, name] = hit;
    const seen = icons.find((e) => e.icon === icon && e.name === name);
    if (seen) seen.files.push(file);
    else icons.push({ icon, name, files: [file] });
  }
  return { icons, others };
}

/** The badge for a repo's version control: its command name, or nothing for
 *  Git — the branch beside the name already says it's versioned, and a badge
 *  on almost every row says nothing. A label added in rules.yaml shows as is. */
export function vcsBadge(label: string | null | undefined): string | null {
  switch (label) {
    case null:
    case undefined:
    case "":
    case "Git":
      return null;
    case "Jujutsu":
      return "jj";
    case "Mercurial":
      return "hg";
    case "Subversion":
      return "svn";
    case "Fossil":
      return "fossil";
    case "Breezy":
      return "brz";
    case "Darcs":
      return "darcs";
    default:
      return label;
  }
}
