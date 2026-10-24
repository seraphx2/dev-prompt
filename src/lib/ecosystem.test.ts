import { describe, expect, it } from "vitest";
import { ecosystems, vcsBadge } from "./ecosystem";
import { icons } from "./icons";

describe("ecosystems", () => {
  it("groups a repo's project files into one icon per ecosystem", () => {
    const { icons: found, others } = ecosystems(["Dockerfile", "docker-compose.yml", "package.json", "sln", "slnx"]);
    expect(found.map((e) => [e.icon, e.files])).toEqual([
      ["docker", ["Dockerfile", "docker-compose.yml"]],
      ["node", ["package.json"]],
      ["dotnet", ["sln", "slnx"]],
    ]);
    expect(others).toEqual([]);
  });

  it("keeps a file no ecosystem claims as text", () => {
    expect(ecosystems(["Makefile", "Cargo.toml"])).toEqual({
      icons: [{ icon: "rust", name: "Rust", files: ["Cargo.toml"] }],
      others: ["Makefile"],
    });
    expect(ecosystems([])).toEqual({ icons: [], others: [] });
  });

  it("only names icons that exist", () => {
    const files = [
      "package.json", "deno.json", "Cargo.toml", "go.mod", "pyproject.toml", "sln", "Dockerfile", "pom.xml",
      "build.gradle", "CMakeLists.txt", "Gemfile", "composer.json", "mix.exs", "pubspec.yaml", "flake.nix",
      ".project", "code-workspace", ".git",
    ];
    for (const e of ecosystems(files).icons) expect(icons[e.icon], e.icon).toBeDefined();
    expect(ecosystems(files).others).toEqual([]);
  });
});

describe("vcsBadge", () => {
  it("is the short command name, and nothing for Git", () => {
    expect(vcsBadge("Git")).toBe(null);
    expect(vcsBadge(null)).toBe(null);
    expect(vcsBadge("Jujutsu")).toBe("jj");
    expect(vcsBadge("Mercurial")).toBe("hg");
    expect(vcsBadge("Subversion")).toBe("svn");
    expect(vcsBadge("Fossil")).toBe("fossil");
    expect(vcsBadge("Breezy")).toBe("brz");
    expect(vcsBadge("Darcs")).toBe("darcs");
    expect(vcsBadge("Pijul")).toBe("Pijul");
  });
});
