//! `go.work` workspace parsing for the `go-work` provider.
//!
//! Lists the module directories a `go.work` `use`s so each can get its own
//! `go build` / `go test` (run from that module's dir). Text parse, no `go`
//! subprocess.

use std::path::{Path, PathBuf};

/// Cap on modules surfaced from one workspace.
const MAX: usize = 16;

fn strip_comment(s: &str) -> &str {
    match s.find("//") {
        Some(i) => &s[..i],
        None => s,
    }
}

fn push_use(base: &Path, tok: &str, out: &mut Vec<(String, PathBuf)>) {
    let tok = tok.trim().trim_matches('"');
    if tok.is_empty() || tok == "(" || tok == ")" {
        return;
    }
    let dir = base.join(tok.replace('\\', "/"));
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| *s != "." && !s.is_empty())
        .unwrap_or(tok)
        .to_string();
    out.push((name, dir));
}

/// `(module name, module dir)` for every `use` directive in `<dir>/go.work`.
pub fn modules(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(text) = std::fs::read_to_string(dir.join("go.work")) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if in_block {
            if line == ")" {
                in_block = false;
            } else {
                push_use(dir, line, &mut out);
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("use") else {
            continue;
        };
        // `use` must be its own token, not a prefix ("used", "useful", …).
        if !rest.starts_with(|c: char| c.is_whitespace() || c == '(') {
            continue;
        }
        let rest = rest.trim_start();
        if let Some(inner) = rest.strip_prefix('(') {
            let inner = inner.trim();
            if inner.is_empty() {
                in_block = true;
            } else {
                // single-line `use ( ./a ./b )`
                for tok in inner.trim_end_matches(')').split_whitespace() {
                    push_use(dir, tok, &mut out);
                }
                in_block = !inner.ends_with(')');
            }
        } else {
            push_use(dir, rest, &mut out);
        }
    }

    out.truncate(MAX);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "dp-gowork-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_single_and_block_use_directives() {
        let d = scratch();
        fs::write(
            d.join("go.work"),
            "go 1.22\n\
             \n\
             use ./api\n\
             \n\
             use (\n\
             \t./services/worker // background jobs\n\
             \t./tools/gen\n\
             )\n\
             \n\
             replace example.com/x => ./vendor/x\n",
        )
        .unwrap();

        let mods = modules(&d);
        let names: Vec<&str> = mods.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["api", "worker", "gen"]);
        assert!(mods[1].1.ends_with("services/worker"));
        // `replace` directive is not a module
        assert!(!mods.iter().any(|(n, _)| n == "x"));

        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_file_yields_nothing() {
        assert!(modules(Path::new("/no/such/dir")).is_empty());
    }
}
