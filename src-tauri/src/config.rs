use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const APP_DIR: &str = "dev-prompt";
/// UI-managed settings (hotkey, roots, scan, cache lifetime).
pub const CONFIG_FILE: &str = "config.yaml";
/// Hand-authored rule-engine overrides (markers, programs, rules, universal).
pub const RULES_FILE: &str = "rules.yaml";

/// The bundled defaults, embedded at compile time. `config.yaml` supplies
/// settings and `rules.yaml` supplies overrides, both layered on top — see
/// [`merge_settings`] and [`merge_overrides`].
const DEFAULT_CONFIG_YAML: &str = include_str!("default_config.yaml");

/// Scaffold written to `rules.yaml` on first run — structure plus commented
/// examples; parses as an empty override set.
const RULES_TEMPLATE: &str = include_str!("rules_template.yaml");

// --- runtime (merged) config ------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Config {
    /// Accelerator string understood by `tauri-plugin-global-shortcut`.
    pub hotkey: String,
    /// Root directories to scan. `~`, `%VAR%`, `$VAR` are expanded.
    pub roots: Vec<String>,
    pub scan: ScanConfig,
    /// How long a cached repo list stays fresh.
    pub cache_ttl_secs: u64,
    /// Discovery markers — "this folder is a project". Every `rules[].match`
    /// also counts, so a rule implies a marker.
    pub markers: Vec<Marker>,
    /// Named program-resolution recipes, referenced from rules as `{{key}}`.
    pub programs: BTreeMap<String, ProgramSpec>,
    /// Matcher → action rules.
    pub rules: Vec<Rule>,
    /// Always-available actions.
    pub universal: UniversalConfig,
    /// Built-in rules the user turned off (kept for the settings viewer, never
    /// evaluated).
    #[serde(skip)]
    pub disabled_rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        // Only used if the embedded default_config.yaml fails to parse (a test
        // guards against that).
        Config {
            hotkey: "CmdOrCtrl+Shift+Space".into(),
            roots: Vec::new(),
            scan: ScanConfig::default(),
            cache_ttl_secs: 900,
            markers: Vec::new(),
            programs: BTreeMap::new(),
            rules: Vec::new(),
            universal: UniversalConfig::default(),
            disabled_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ScanConfig {
    pub max_depth: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig { max_depth: 4 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Marker {
    Name(String),
    Detail {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}

impl Marker {
    pub fn name(&self) -> &str {
        match self {
            Marker::Name(n) => n,
            Marker::Detail { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ProgramSpec {
    pub any: Vec<ProgramCandidate>,
    pub windows: Vec<ProgramCandidate>,
    pub linux: Vec<ProgramCandidate>,
    pub macos: Vec<ProgramCandidate>,
}

impl ProgramSpec {
    /// Candidates that apply to the current OS, `any` first.
    pub fn candidates(&self) -> Vec<&ProgramCandidate> {
        let mut v: Vec<&ProgramCandidate> = self.any.iter().collect();
        #[cfg(windows)]
        v.extend(self.windows.iter());
        #[cfg(target_os = "linux")]
        v.extend(self.linux.iter());
        #[cfg(target_os = "macos")]
        v.extend(self.macos.iter());
        v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgramCandidate {
    /// A path, exe name (PATH lookup), or filesystem glob. `~` / `%VAR%` expand.
    Path(String),
    /// Windows only: run `vswhere.exe <args>` and use the stdout path.
    Vswhere { vswhere: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Project,
    Repo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchSpec {
    One(String),
    Many(Vec<String>),
}

impl Default for MatchSpec {
    fn default() -> Self {
        MatchSpec::Many(Vec::new())
    }
}

impl MatchSpec {
    pub fn globs(&self) -> Vec<&str> {
        match self {
            MatchSpec::One(s) => vec![s.as_str()],
            MatchSpec::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Rule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "match")]
    pub match_: MatchSpec,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    pub per_file: bool,
    pub scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manager: Option<String>,
    /// Program keys that must resolve for this rule to contribute anything.
    pub needs: Vec<String>,
    /// Bare executables that must be on `PATH`.
    pub requires: Vec<String>,
    pub actions: Vec<RuleAction>,
    /// A rule carrying only `disable: <id>` removes that built-in rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct RuleAction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// Free-form command line (quote-aware split). Mutually exclusive with `program`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub args: Vec<String>,
    /// Run inside the resolved terminal at the working dir.
    pub terminal: bool,
    /// Handled in the frontend (e.g. copy path). No process spawned.
    pub client: bool,
    pub needs: Vec<String>,
    /// The action `Enter` runs on a repo (universal actions only).
    pub default: bool,
}

impl RuleAction {
    pub fn action_id(&self) -> String {
        self.id.clone().unwrap_or_else(|| slug(&self.name))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct UniversalConfig {
    pub actions: Vec<RuleAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Built-ins the user turned off — kept for the settings viewer.
    #[serde(skip)]
    pub disabled: Vec<RuleAction>,
}

// --- user config (the on-disk file) ---------------------------------------

/// `config.yaml` — the settings the Settings screen manages. No rule-engine
/// keys live here; those are in `rules.yaml` ([`RuleOverrides`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct UserConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan: Option<ScanConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_ttl_secs: Option<u64>,
}

/// `rules.yaml` — hand-authored overrides layered over the bundled defaults.
/// The Settings screen never rewrites this file, so comments survive.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct RuleOverrides {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub markers_replace: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub programs: BTreeMap<String, ProgramSpec>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules_disable: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub universal: Option<UniversalPatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct UniversalPatch {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<RuleAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

// --- loading / merging ----------------------------------------------------

pub fn bundled_defaults() -> Config {
    serde_yaml_ng::from_str(DEFAULT_CONFIG_YAML)
        .expect("bundled default_config.yaml must parse")
}

/// Apply `config.yaml` — plain scalar settings, present values win.
fn merge_settings(cfg: &mut Config, u: UserConfig) {
    if let Some(h) = u.hotkey {
        cfg.hotkey = h;
    }
    if !u.roots.is_empty() {
        cfg.roots = u.roots;
    }
    if let Some(s) = u.scan {
        cfg.scan = s;
    }
    if let Some(t) = u.cache_ttl_secs {
        cfg.cache_ttl_secs = t;
    }
}

/// Apply `rules.yaml` — the rule-engine overrides.
fn merge_overrides(cfg: &mut Config, u: RuleOverrides) {
    if u.markers_replace {
        cfg.markers = u.markers;
    } else {
        cfg.markers.extend(u.markers);
    }

    for (k, v) in u.programs {
        cfg.programs.insert(k, v);
    }

    // User rules take precedence and show first.
    if !u.rules.is_empty() {
        let mut rules = u.rules;
        rules.append(&mut cfg.rules);
        cfg.rules = rules;
    }

    // Move built-ins named by `rules_disable` or a bare `disable:` rule aside
    // (kept for the settings viewer, never evaluated).
    let mut disabled: HashSet<String> = u.rules_disable.into_iter().collect();
    disabled.extend(cfg.rules.iter().filter_map(|r| r.disable.clone()));
    if !disabled.is_empty() {
        let mut kept = Vec::with_capacity(cfg.rules.len());
        for r in cfg.rules.drain(..) {
            if r.disable.is_some() {
                continue; // bare disable-marker rule — no content to keep
            }
            if r.id.as_deref().is_some_and(|id| disabled.contains(id)) {
                cfg.disabled_rules.push(r);
            } else {
                kept.push(r);
            }
        }
        cfg.rules = kept;
    }

    if let Some(p) = u.universal {
        if !p.disable.is_empty() {
            let dis: HashSet<&String> = p.disable.iter().collect();
            let mut kept = Vec::with_capacity(cfg.universal.actions.len());
            for a in cfg.universal.actions.drain(..) {
                if dis.contains(&a.action_id()) {
                    cfg.universal.disabled.push(a);
                } else {
                    kept.push(a);
                }
            }
            cfg.universal.actions = kept;
        }
        cfg.universal.actions.extend(p.add);
        if p.default.is_some() {
            cfg.universal.default = p.default;
        }
    }
}

/// Load the merged runtime config, writing a starter `config.yaml` and a
/// `rules.yaml` scaffold on first run.
pub fn load() -> AppResult<Config> {
    let mut cfg = bundled_defaults();

    let settings_path = config_path()?;
    if !settings_path.exists() {
        save_user(&first_run_user())?;
    }
    if let Ok(text) = std::fs::read_to_string(&settings_path) {
        let user: UserConfig = serde_yaml_ng::from_str(&text)?;
        merge_settings(&mut cfg, user);
    }

    let overrides_path = rules_path()?;
    if !overrides_path.exists() {
        std::fs::write(&overrides_path, RULES_TEMPLATE)?;
    }
    if let Ok(text) = std::fs::read_to_string(&overrides_path) {
        let ov: RuleOverrides = serde_yaml_ng::from_str(&text)?;
        merge_overrides(&mut cfg, ov);
    }

    Ok(cfg)
}

/// The raw settings file (for the settings screen), or a starter if none exists.
pub fn load_user() -> AppResult<UserConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(first_run_user());
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_yaml_ng::from_str(&text)?)
}

pub fn save_user(u: &UserConfig) -> AppResult<()> {
    let path = config_path()?;
    let header = "# dev-prompt settings — managed by the Settings screen. Rule\n\
                  # overrides (markers / programs / rules / universal) live in\n\
                  # rules.yaml. Reference:\n\
                  # https://github.com/seraphx2/dev-prompt/blob/main/docs/configuration.md\n\n";
    let body = serde_yaml_ng::to_string(u)?;
    std::fs::write(&path, format!("{header}{body}"))?;
    Ok(())
}

fn first_run_user() -> UserConfig {
    UserConfig {
        hotkey: Some("CmdOrCtrl+Shift+Space".into()),
        roots: default_roots(),
        scan: Some(ScanConfig::default()),
        cache_ttl_secs: Some(900),
    }
}

fn default_roots() -> Vec<String> {
    // Harmless extras are filtered by `resolved_roots`.
    #[cfg(windows)]
    {
        vec![
            "%USERPROFILE%\\source\\repos".into(),
            "~/git".into(),
            "~/src".into(),
            "~/code".into(),
            "~/projects".into(),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![
            "~/src".into(),
            "~/git".into(),
            "~/code".into(),
            "~/projects".into(),
        ]
    }
}

// --- discovery helpers --------------------------------------------------

/// Every glob that makes a directory count as a project: markers ∪ rule matchers.
pub fn discovery_globs(cfg: &Config) -> Vec<String> {
    let mut out: Vec<String> = cfg.markers.iter().map(|m| m.name().to_string()).collect();
    for r in &cfg.rules {
        for g in r.match_.globs() {
            out.push(g.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

// --- paths & expansion -------------------------------------------------

pub fn config_dir() -> AppResult<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| AppError::msg("no OS config directory"))?
        .join(APP_DIR);
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

pub fn cache_dir() -> AppResult<PathBuf> {
    let base = dirs::cache_dir()
        .ok_or_else(|| AppError::msg("no OS cache directory"))?
        .join(APP_DIR);
    std::fs::create_dir_all(&base)?;
    Ok(base)
}

pub fn config_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

pub fn rules_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join(RULES_FILE))
}

/// Expand `~`, `%VAR%` (Windows-style) and `$VAR` (POSIX-style).
pub fn expand_path(raw: &str) -> PathBuf {
    let mut s = raw.trim().to_string();

    if let Some(rest) = s.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            let rest = rest.trim_start_matches(['/', '\\']);
            return home.join(rest);
        }
    }

    while let Some(start) = s.find('%') {
        if let Some(end_rel) = s[start + 1..].find('%') {
            let end = start + 1 + end_rel;
            let name = &s[start + 1..end];
            let val = std::env::var(name).unwrap_or_default();
            s.replace_range(start..=end, &val);
        } else {
            break;
        }
    }

    while let Some(start) = s.find('$') {
        let rest = &s[start + 1..];
        let end_rel = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if end_rel == 0 {
            break;
        }
        let name = &rest[..end_rel];
        let val = std::env::var(name).unwrap_or_default();
        s.replace_range(start..start + 1 + end_rel, &val);
    }

    PathBuf::from(s)
}

/// Same expansion as [`expand_path`] but returns a string (for glob patterns).
pub fn expand_str(raw: &str) -> String {
    expand_path(raw).to_string_lossy().into_owned()
}

/// Expand + keep only roots that currently exist as directories.
pub fn resolved_roots(cfg: &Config) -> Vec<PathBuf> {
    cfg.roots
        .iter()
        .map(|r| expand_path(r))
        .filter(|p| p.is_dir())
        .collect()
}

/// kebab-case slug for auto-deriving action ids from names.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_default_config_parses() {
        let cfg = bundled_defaults();
        assert!(!cfg.rules.is_empty(), "defaults should ship rules");
        assert!(!cfg.programs.is_empty(), "defaults should ship programs");
        assert!(!cfg.universal.actions.is_empty());
    }

    #[test]
    fn bundled_defaults_carry_the_fattened_rules_and_programs() {
        let cfg = bundled_defaults();

        let has_rule = |id: &str| cfg.rules.iter().any(|r| r.id.as_deref() == Some(id));
        for id in [
            "maven", "gradle", "cmake", "composer", "mix", "deno", "nix-flake",
            "docker-image", "csproj-editors", "eclipse-project",
        ] {
            assert!(has_rule(id), "missing bundled rule `{id}`");
        }

        for key in ["cursor", "zed", "idea", "goland", "lazygit", "anypoint", "eclipse", "sts"] {
            assert!(cfg.programs.contains_key(key), "missing bundled program `{key}`");
        }

        let has_universal = |id: &str| cfg.universal.actions.iter().any(|a| a.action_id() == id);
        assert!(has_universal("cursor") && has_universal("gitkraken") && has_universal("lazygit"));

        // A rule's `match` doubles as a discovery marker.
        let globs = discovery_globs(&cfg);
        for g in ["pom.xml", "Dockerfile", "composer.json", "go.work"] {
            assert!(globs.iter().any(|x| x == g), "`{g}` should be a discovery glob");
        }
    }

    #[test]
    fn settings_file_overrides_scalars() {
        let mut cfg = bundled_defaults();
        let user: UserConfig =
            serde_yaml_ng::from_str("hotkey: Alt+Space\nscan:\n  max_depth: 7\n").unwrap();
        merge_settings(&mut cfg, user);
        assert_eq!(cfg.hotkey, "Alt+Space");
        assert_eq!(cfg.scan.max_depth, 7);
    }

    #[test]
    fn rules_file_prepends_and_disables() {
        let mut cfg = bundled_defaults();
        let base = cfg.rules.len();
        let ov: RuleOverrides = serde_yaml_ng::from_str(
            r#"
rules:
  - match: "Makefile"
    actions: [{ name: "make", run: "make", terminal: true }]
rules_disable: [docker-image]
"#,
        )
        .unwrap();
        merge_overrides(&mut cfg, ov);
        assert_eq!(cfg.rules[0].actions[0].name, "make"); // user rule is first
        assert_eq!(cfg.rules.len(), base + 1 - 1); // +make, -docker-image
        assert!(cfg
            .disabled_rules
            .iter()
            .any(|r| r.id.as_deref() == Some("docker-image")));
    }

    #[test]
    fn rules_template_parses_as_an_empty_override_set() {
        let ov: RuleOverrides = serde_yaml_ng::from_str(RULES_TEMPLATE).unwrap();
        assert!(ov.markers.is_empty());
        assert!(ov.programs.is_empty());
        assert!(ov.rules.is_empty());
        assert!(ov.rules_disable.is_empty());
        assert!(ov.universal.is_none());
    }

    #[test]
    fn expands_tilde_and_env() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_path("~/foo"), home.join("foo"));
        std::env::set_var("DP_CFG_TEST", "xyz");
        assert_eq!(expand_path("a/%DP_CFG_TEST%/b"), PathBuf::from("a/xyz/b"));
        assert_eq!(expand_path("a/$DP_CFG_TEST/b"), PathBuf::from("a/xyz/b"));
    }

    #[test]
    fn unmatched_percent_is_left_alone() {
        assert_eq!(expand_path("100%done"), PathBuf::from("100%done"));
    }

    #[test]
    fn slug_basics() {
        assert_eq!(slug("Open in VS Code"), "open-in-vs-code");
        assert_eq!(slug("cargo run!"), "cargo-run");
    }
}
