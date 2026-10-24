//! Task-runner targets for the `make`, `just` and `task` providers.
//!
//! Each reads its file and lists what can be run by name — one terminal
//! action per target, run from the project's directory. Text parses, no
//! subprocess: they run on every menu open. (Finding go-task's binary asks it
//! once per run — see [`go_task`].)

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

/// Cap on targets surfaced from one file.
const MAX: usize = 20;

/// The first of `names` that exists in `dir`, read.
fn read_first(dir: &Path, names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| std::fs::read_to_string(dir.join(n)).ok())
}

/// A Makefile's targets, in file order. When it declares `.PHONY` targets
/// those are its commands (`build`, `test`, `clean`) and the rest are files it
/// makes, so only the phony ones are listed; otherwise every plain named
/// target. Pattern (`%.o`), variable (`$(OUT)`), path (`bin/app`) and special
/// (`.DEFAULT`) targets are never runnable by name, and assignments
/// (`CC := gcc`, `X = a:b`) aren't rules.
pub fn make_targets(dir: &Path) -> Vec<String> {
    let Some(text) = read_first(dir, &["GNUmakefile", "makefile", "Makefile"]) else {
        return Vec::new();
    };
    // A line ending in `\` goes on into the next: a `.PHONY` list or an
    // assignment spread over several lines is one logical line.
    let text = text.replace("\r\n", "\n").replace("\\\n", " ");
    let mut targets: Vec<String> = Vec::new();
    let mut phony: Vec<String> = Vec::new();
    let mut in_define = false;
    for line in text.lines() {
        // Recipe lines are tab-indented; conditionals and includes have no
        // rule colon, and `define … endef` holds text, not rules.
        if line.starts_with('\t') {
            continue;
        }
        let line = line.split('#').next().unwrap_or("").trim_end();
        let word = line.split_whitespace().next().unwrap_or("");
        if in_define {
            in_define = word != "endef";
            continue;
        }
        if word == "define" {
            in_define = true;
            continue;
        }
        let Some(colon) = line.find(':') else { continue };
        let (head, rest) = (&line[..colon], &line[colon + 1..]);
        if head.contains('=') || rest.starts_with('=') || rest.starts_with(":=") {
            continue;
        }
        if head.trim() == ".PHONY" {
            phony.extend(rest.split_whitespace().map(String::from));
            continue;
        }
        for t in head.split_whitespace() {
            let runnable = !t.starts_with('.') && !t.contains(['%', '$', '/', '\\', '(']);
            if runnable && !targets.iter().any(|x| x == t) {
                targets.push(t.to_string());
            }
        }
    }
    if !phony.is_empty() {
        targets.retain(|t| phony.contains(t));
    }
    targets.truncate(MAX);
    targets
}

/// go-task's binary, by the name to run it with: `go-task` where a distro ships
/// it so, else `task` — but only once it says it's go-task, since on Linux
/// `task` is usually Taskwarrior. Asked once per run (and again after a
/// rescan, see [`forget`]); `None` hides the Taskfile targets.
pub fn go_task() -> Option<String> {
    let mut found = GO_TASK.lock().unwrap();
    if let Some(known) = found.as_ref() {
        return known.clone();
    }
    let answer = if crate::rules::which("go-task").is_some() {
        Some("go-task".to_string())
    } else if crate::rules::which("task").is_some() {
        let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
        crate::provision::run_captured("task", &["--help".into()], &home, Duration::from_secs(5))
            .ok()
            .filter(|help| help.to_lowercase().contains("taskfile"))
            .map(|_| "task".to_string())
    } else {
        None
    };
    *found = Some(answer.clone());
    answer
}

static GO_TASK: Mutex<Option<Option<String>>> = Mutex::new(None);

/// Forget which `task` is go-task's (a tool installed or removed since).
pub fn forget() {
    *GO_TASK.lock().unwrap() = None;
}

/// A justfile's recipes, in file order. Private ones (`_name`, or under a
/// `[private]` attribute) are helpers, and a recipe with a required
/// parameter (`deploy env:`) would only fail when run bare — both left out.
pub fn just_recipes(dir: &Path) -> Vec<String> {
    let Some(text) = read_first(dir, &["justfile", "Justfile", ".justfile"]) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut private = false;
    for line in text.lines() {
        if line.starts_with([' ', '\t']) || line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            private |= line.contains("private");
            continue;
        }
        let was_private = std::mem::take(&mut private);
        let Some((name, params)) = recipe_header(line) else { continue };
        if was_private || name.starts_with('_') || has_required_param(params) {
            continue;
        }
        out.push(name.to_string());
        if out.len() == MAX {
            break;
        }
    }
    out
}

/// `(name, parameters)` when `line` is a recipe header (`@name a b='x': deps`),
/// not a setting, alias, assignment or import.
fn recipe_header(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix('@').unwrap_or(line);
    let end = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(line.len());
    let name = &line[..end];
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    // The recipe's colon is the first one outside quotes (a default value can
    // hold one); `:=` makes it an assignment instead.
    let rest = &line[end..];
    let mut quote = None;
    for (i, c) in rest.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, ':') => {
                return (!rest[i + 1..].starts_with('=')).then(|| (name, &rest[..i]));
            }
            _ => {}
        }
    }
    None
}

/// Whether a recipe's parameter list has one without a default: `env`, `+files`
/// (one or more) and `$env` need a value; `env='dev'` and `*args` don't.
fn has_required_param(params: &str) -> bool {
    let mut quote = None;
    let mut token = String::new();
    let mut required = false;
    let mut finish = |t: &mut String| {
        if !t.is_empty() && !t.contains('=') && !t.starts_with('*') {
            required = true;
        }
        t.clear();
    };
    for c in params.chars() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, c) if c.is_whitespace() => finish(&mut token),
            _ => token.push(c),
        }
    }
    finish(&mut token);
    required
}

/// A Taskfile's tasks, in file order, minus the `internal: true` ones (helpers
/// other tasks call). Tasks from `includes:` live in other files — not listed.
pub fn taskfile_tasks(dir: &Path) -> Vec<String> {
    let Some(text) = read_first(
        dir,
        &[
            "Taskfile.yml", "taskfile.yml", "Taskfile.yaml", "taskfile.yaml",
            "Taskfile.dist.yml", "taskfile.dist.yml", "Taskfile.dist.yaml", "taskfile.dist.yaml",
        ],
    ) else {
        return Vec::new();
    };
    let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
        return Vec::new();
    };
    let Some(tasks) = doc.get("tasks").and_then(|t| t.as_mapping()) else {
        return Vec::new();
    };
    tasks
        .iter()
        .filter(|(_, body)| body.get("internal").and_then(|v| v.as_bool()) != Some(true))
        .filter_map(|(name, _)| name.as_str().map(String::from))
        .take(MAX)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "dp-tasks-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn makefile_lists_its_phony_commands() {
        let d = scratch("make");
        fs::write(
            d.join("Makefile"),
            "CC := gcc\n\
             OUT = bin/app\n\
             URL = http://example.com\n\
             .PHONY: build test clean\n\
             \n\
             build: $(OUT) ## compile\n\
             \t$(CC) -o $(OUT) main.c\n\
             $(OUT): main.o\n\
             \t@echo link\n\
             %.o: %.c\n\
             \t$(CC) -c $<\n\
             test clean:\n\
             \t@echo run\n\
             main.o: main.c\n\
             define HELP\n\
             usage: make build\n\
             endef\n\
             .DEFAULT_GOAL := build\n",
        )
        .unwrap();
        assert_eq!(make_targets(&d), ["build", "test", "clean"]);

        // A .PHONY list continued over lines; an assignment continued onto a
        // line with a colon isn't a rule.
        fs::write(
            d.join("Makefile"),
            ".PHONY: build \\\n\ttest clean\nSRCS = a.c \\\n    b:c\nbuild test:\n\techo\nclean:\n\techo\n",
        )
        .unwrap();
        assert_eq!(make_targets(&d), ["build", "test", "clean"]);

        // Without .PHONY every plain named target counts — files included.
        fs::write(d.join("Makefile"), "all: app\napp: main.c\n\tcc main.c\nx = a:b\n").unwrap();
        assert_eq!(make_targets(&d), ["all", "app"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn justfile_lists_recipes_that_run_bare() {
        let d = scratch("just");
        fs::write(
            d.join("justfile"),
            "set shell := [\"bash\", \"-c\"]\n\
             alias b := build\n\
             version := \"1.2\"\n\
             import 'other.just'\n\
             \n\
             # the default\n\
             default: build\n\
             \n\
             @build mode='release':\n\
             \x20   cargo build --{{mode}}\n\
             \n\
             test *args: build\n\
             \x20   cargo test {{args}}\n\
             \n\
             deploy env:\n\
             \x20   ./deploy {{env}}\n\
             \n\
             serve port='80' host=\"a:b\":\n\
             \x20   ./serve\n\
             \n\
             _helper:\n\
             \x20   echo hi\n\
             \n\
             [private]\n\
             hidden:\n\
             \x20   echo hidden\n\
             \n\
             [group('ci')]\n\
             lint:\n\
             \x20   cargo clippy\n",
        )
        .unwrap();
        assert_eq!(just_recipes(&d), ["default", "build", "test", "serve", "lint"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn taskfile_lists_its_public_tasks() {
        let d = scratch("task");
        fs::write(
            d.join("Taskfile.yml"),
            "version: '3'\n\
             includes:\n\
             \x20 docs: ./docs\n\
             tasks:\n\
             \x20 build:\n\
             \x20   cmds: [go build ./...]\n\
             \x20 test: go test ./...\n\
             \x20 setup:\n\
             \x20   internal: true\n\
             \x20   cmds: [echo setup]\n\
             \x20 'db:migrate':\n\
             \x20   cmds: [./migrate]\n",
        )
        .unwrap();
        assert_eq!(taskfile_tasks(&d), ["build", "test", "db:migrate"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_files_yield_nothing() {
        let d = Path::new("/no/such/dir");
        assert!(make_targets(d).is_empty() && just_recipes(d).is_empty() && taskfile_tasks(d).is_empty());
    }
}
