//! .NET solution / project enumeration for the `dotnet` provider.
//!
//! Reads a `.sln` (plain text) or `.slnx` (flat XML) and lists the buildable
//! `.csproj` / `.vbproj` / `.fsproj` it references, plus any stray project file
//! sitting next to it that no solution covers. No `dotnet` subprocess — action
//! building must stay fast and offline; the format is simple enough to parse.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Cap on projects surfaced from one directory, so a 60-project solution doesn't
/// flood the action menu.
const MAX_UNITS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The project's file stem, e.g. `MyApp` or `MyApp.Tests`.
    pub name: String,
    /// Absolute path to the `.csproj` / `.vbproj` / `.fsproj`.
    pub path: PathBuf,
    /// Name looks like a test project — offer `dotnet test` as well.
    pub is_test: bool,
}

fn has_ext(name: &str, ext: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn is_project_file(name: &str) -> bool {
    has_ext(name, "csproj") || has_ext(name, "vbproj") || has_ext(name, "fsproj")
}

/// `.sln` / `.slnx` paths use Windows separators; normalise before joining.
fn join_rel(dir: &Path, rel: &str) -> PathBuf {
    dir.join(rel.replace('\\', "/"))
}

fn mk_unit(path: PathBuf) -> Unit {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();
    let lower = name.to_lowercase();
    let is_test = lower.ends_with("test") || lower.ends_with("tests");
    Unit {
        name,
        path,
        is_test,
    }
}

/// Buildable .NET projects reachable from `dir`. `files` is `dir`'s top-level
/// entry names (already gathered by `inspect`).
pub fn units(dir: &Path, files: &[String]) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // Solutions first, so a stray project file they already list is not repeated.
    for f in files {
        let (sln, slnx) = (has_ext(f, "sln"), has_ext(f, "slnx"));
        if !sln && !slnx {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(dir.join(f)) else {
            continue;
        };
        let rels = if slnx {
            parse_slnx(&text)
        } else {
            parse_sln(&text)
        };
        for rel in rels {
            let abs = join_rel(dir, &rel);
            if out.len() < MAX_UNITS && seen.insert(abs.clone()) {
                out.push(mk_unit(abs));
            }
        }
    }

    for f in files {
        if is_project_file(f) {
            let abs = dir.join(f);
            if out.len() < MAX_UNITS && seen.insert(abs.clone()) {
                out.push(mk_unit(abs));
            }
        }
    }

    out
}

/// Relative project paths from a `.sln`. Lines look like:
/// `Project("{type}") = "Name", "rel\path.csproj", "{id}"` — keep the second
/// quoted field when it points at a project file (drops "solution folder" rows,
/// whose second field is just a name).
fn parse_sln(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            if !line.starts_with("Project(") {
                return None;
            }
            let rhs = line.split_once('=')?.1;
            let path = quoted_fields(rhs).into_iter().nth(1)?;
            is_project_file(path).then(|| path.to_string())
        })
        .collect()
}

/// Contents of each `"..."` run in `s`, in order.
fn quoted_fields(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(&rest[..close]);
        rest = &rest[close + 1..];
    }
    out
}

/// Relative project paths from a `.slnx` — the `Path` attribute of every
/// `<Project …/>` element. Not a full XML parse; the format is flat.
fn parse_slnx(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("<Project") {
        rest = &rest[pos + "<Project".len()..];
        let end = rest.find('>').unwrap_or(rest.len());
        if let Some(p) = tag_attr(&rest[..end], "Path") {
            if is_project_file(p) {
                out.push(p.to_string());
            }
        }
        rest = &rest[end..];
    }
    out
}

fn tag_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(name)?;
    let after = tag[at + name.len()..].trim_start().strip_prefix('=')?;
    let after = after.trim_start().strip_prefix('"')?;
    let close = after.find('"')?;
    Some(&after[..close])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "dp-dotnet-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_sln_projects_and_skips_solution_folders() {
        let sln = "\
Microsoft Visual Studio Solution File, Format Version 12.00
Project(\"{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}\") = \"App\", \"src\\App\\App.csproj\", \"{11}\"
Project(\"{2150E333-8FDC-42A3-9474-1A3956D46DE8}\") = \"Solution Items\", \"Solution Items\", \"{22}\"
Project(\"{6EC3EE1D-3C4E-46DD-8F32-0CC8E7565705}\") = \"App.Tests\", \"tests\\App.Tests\\App.Tests.fsproj\", \"{33}\"
";
        assert_eq!(
            parse_sln(sln),
            vec!["src\\App\\App.csproj", "tests\\App.Tests\\App.Tests.fsproj"]
        );
    }

    #[test]
    fn parses_slnx_projects_and_ignores_files_and_folders() {
        let slnx = "\
<Solution>
  <Project Path=\"src/App/App.csproj\" />
  <Folder Name=\"/Docs/\">
    <File Path=\"README.md\" />
  </Folder>
  <Project Path=\"tests/App.Tests/App.Tests.csproj\" Type=\"Test\" />
</Solution>";
        assert_eq!(
            parse_slnx(slnx),
            vec!["src/App/App.csproj", "tests/App.Tests/App.Tests.csproj"]
        );
    }

    #[test]
    fn units_merges_solution_with_stray_project_and_dedupes() {
        let d = scratch("units");
        fs::write(
            d.join("My.sln"),
            "Project(\"{X}\") = \"App\", \"App\\App.csproj\", \"{Y}\"\n\
             Project(\"{X}\") = \"Tool\", \"Tool.csproj\", \"{Z}\"\n",
        )
        .unwrap();
        fs::write(d.join("Tool.csproj"), "").unwrap(); // also listed in the .sln

        let files = vec!["My.sln".to_string(), "Tool.csproj".to_string()];
        let mut names: Vec<_> = units(&d, &files).into_iter().map(|u| u.name).collect();
        names.sort();
        assert_eq!(names, vec!["App", "Tool"]); // Tool once, not twice

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn flags_test_projects_by_name() {
        assert!(mk_unit(PathBuf::from("/x/Foo.Tests.csproj")).is_test);
        assert!(mk_unit(PathBuf::from("/x/FooTest.fsproj")).is_test);
        assert!(!mk_unit(PathBuf::from("/x/Foo.csproj")).is_test);
    }
}
