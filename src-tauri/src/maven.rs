//! Maven multi-module parsing for the `maven-modules` provider.
//!
//! Lists the `<module>` directories a reactor `pom.xml` declares so each can get
//! its own `mvn` run (from that module's dir). Flat text scan for `<module>`
//! elements — not a full XML parse; that's all the reactor list needs.

use std::path::{Path, PathBuf};

/// Cap on modules surfaced from one reactor pom.
const MAX: usize = 20;

/// `(module name, module dir)` for every `<module>…</module>` in `<dir>/pom.xml`.
pub fn modules(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(text) = std::fs::read_to_string(dir.join("pom.xml")) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(open) = rest.find("<module>") {
        rest = &rest[open + "<module>".len()..];
        let Some(close) = rest.find("</module>") else {
            break;
        };
        let rel = rest[..close].trim();
        rest = &rest[close + "</module>".len()..];
        if rel.is_empty() {
            continue;
        }
        let mdir = dir.join(rel.replace('\\', "/"));
        let name = mdir
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(rel)
            .to_string();
        out.push((name, mdir));
        if out.len() >= MAX {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_module_list_and_nested_paths() {
        let d = std::env::temp_dir().join(format!(
            "dp-maven-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("pom.xml"),
            r#"<?xml version="1.0"?>
<project>
  <artifactId>reactor</artifactId>
  <packaging>pom</packaging>
  <modules>
    <module>service-api</module>
    <module>libs/common</module>
  </modules>
</project>
"#,
        )
        .unwrap();

        let mods = modules(&d);
        let names: Vec<&str> = mods.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["service-api", "common"]);
        assert!(mods[1].1.ends_with("libs/common"));

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_file_yields_nothing() {
        assert!(modules(Path::new("/no/such/dir")).is_empty());
    }
}
