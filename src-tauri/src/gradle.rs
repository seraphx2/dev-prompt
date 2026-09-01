//! Gradle multi-project parsing for the `gradle-modules` provider.
//!
//! Reads `settings.gradle` / `settings.gradle.kts` and pulls the paths out of
//! every `include(...)` statement. Gradle is root-centric — actions run as
//! `gradle :path:to:project:task` from the settings dir, not from the module —
//! so this returns the `:`-prefixed project path, not a directory.

use std::collections::HashSet;
use std::path::Path;

/// Cap on projects surfaced from one settings file.
const MAX: usize = 40;

fn strip_comments(src: &str) -> String {
    // Block comments.
    let mut s = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        s.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    s.push_str(rest);
    // Line comments.
    s.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Quoted string arguments of an `include` statement starting at `after`
/// (the text immediately following the `include` keyword). Handles
/// `include 'a', 'b'`, `include("a", "b")` and multi-line parenthesised lists.
fn collect_args(after: &str) -> Vec<String> {
    let bytes = after.as_bytes();
    let n = bytes.len();
    let mut cur = 0usize;
    let skip_ws = |c: &mut usize| {
        while *c < n && (bytes[*c] as char).is_ascii_whitespace() {
            *c += 1;
        }
    };

    skip_ws(&mut cur);
    if cur < n && bytes[cur] == b'(' {
        cur += 1;
    }

    let mut out = Vec::new();
    loop {
        skip_ws(&mut cur);
        if cur >= n {
            break;
        }
        let q = bytes[cur];
        if q != b'\'' && q != b'"' {
            break;
        }
        cur += 1;
        let start = cur;
        while cur < n && bytes[cur] != q {
            cur += 1;
        }
        if cur >= n {
            break;
        }
        out.push(after[start..cur].to_string());
        cur += 1;
        skip_ws(&mut cur);
        if cur < n && bytes[cur] == b',' {
            cur += 1;
            continue;
        }
        break;
    }
    out
}

/// `(project name, `:`-prefixed Gradle project path)` for every `include`d
/// project in `<dir>/settings.gradle[.kts]`.
pub fn projects(dir: &Path) -> Vec<(String, String)> {
    let Some(src) = ["settings.gradle", "settings.gradle.kts"]
        .iter()
        .find_map(|f| std::fs::read_to_string(dir.join(f)).ok())
    else {
        return Vec::new();
    };
    let s = strip_comments(&src);
    let bytes = s.as_bytes();

    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find("include") {
        let at = from + rel;
        from = at + "include".len();

        // Word boundary before, and `(` or whitespace after — excludes
        // `includeBuild`, `includeFlat`, `x.include`.
        if at > 0 {
            let prev = bytes[at - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'.' {
                continue;
            }
        }
        match s[from..].chars().next() {
            Some(c) if c == '(' || c.is_whitespace() => {}
            _ => continue,
        }

        for lit in collect_args(&s[from..]) {
            let path = lit.trim().trim_start_matches(':');
            if path.is_empty() {
                continue;
            }
            let gpath = format!(":{path}");
            if !seen.insert(gpath.clone()) {
                continue;
            }
            let name = path.rsplit(':').next().unwrap_or(path).to_string();
            out.push((name, gpath));
            if out.len() >= MAX {
                return out;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "dp-gradle-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_groovy_includes_single_multi_and_nested() {
        let d = scratch();
        fs::write(
            d.join("settings.gradle"),
            "rootProject.name = 'demo'\n\
             // a comment\n\
             include ':app', 'core'\n\
             include(\n  'features:login',\n  ':libs:util',\n)\n\
             includeBuild '../shared'\n",
        )
        .unwrap();

        let ps = projects(&d);
        let got: Vec<(&str, &str)> =
            ps.iter().map(|(n, p)| (n.as_str(), p.as_str())).collect();
        assert_eq!(
            got,
            vec![
                ("app", ":app"),
                ("core", ":core"),
                ("login", ":features:login"),
                ("util", ":libs:util"),
            ]
        );
        // `includeBuild` is a composite build, not a project
        assert!(!ps.iter().any(|(n, _)| n == "shared"));

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn parses_kotlin_settings() {
        let d = scratch();
        fs::write(
            d.join("settings.gradle.kts"),
            "rootProject.name = \"demo\"\ninclude(\"app\", \"data:remote\")\n",
        )
        .unwrap();
        let ps = projects(&d);
        assert_eq!(
            ps,
            vec![
                ("app".to_string(), ":app".to_string()),
                ("remote".to_string(), ":data:remote".to_string()),
            ]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_file_yields_nothing() {
        assert!(projects(Path::new("/no/such/dir")).is_empty());
    }
}
