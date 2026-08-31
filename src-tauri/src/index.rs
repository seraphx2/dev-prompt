use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::Serialize;

use crate::scan::Repo;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoredRepo {
    pub repo: Repo,
    pub score: u32,
    /// Char indices into `repo.name` that matched (for highlight). Empty when the
    /// match came from the path rather than the name.
    pub match_indices: Vec<u32>,
}

/// Rank `repos` against `query`. An empty query yields every repo ordered by
/// recency then name. A non-empty query keeps only fuzzy matches, best first.
pub fn search(query: &str, repos: &[Repo], limit: usize) -> Vec<ScoredRepo> {
    let query = query.trim();

    if query.is_empty() {
        let mut all: Vec<ScoredRepo> = repos
            .iter()
            .map(|r| ScoredRepo {
                repo: r.clone(),
                score: 0,
                match_indices: Vec::new(),
            })
            .collect();
        all.sort_by(|a, b| {
            b.repo
                .last_seen
                .cmp(&a.repo.last_seen)
                .then_with(|| a.repo.name.to_lowercase().cmp(&b.repo.name.to_lowercase()))
        });
        all.truncate(limit);
        return all;
    }

    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    let mut out: Vec<ScoredRepo> = Vec::new();
    let mut name_buf: Vec<char> = Vec::new();
    let mut path_buf: Vec<char> = Vec::new();
    let mut idx_buf: Vec<u32> = Vec::new();

    for r in repos {
        idx_buf.clear();
        let name_hay = Utf32Str::new(&r.name, &mut name_buf);
        let name_score = pattern.indices(name_hay, &mut matcher, &mut idx_buf);

        if let Some(s) = name_score {
            let mut indices = idx_buf.clone();
            indices.sort_unstable();
            indices.dedup();
            // Name hits are what the user usually means — bias them above path hits.
            out.push(ScoredRepo {
                repo: r.clone(),
                score: s + 1000,
                match_indices: indices,
            });
            continue;
        }

        let path_hay = Utf32Str::new(&r.path, &mut path_buf);
        if let Some(s) = pattern.score(path_hay, &mut matcher) {
            out.push(ScoredRepo {
                repo: r.clone(),
                score: s,
                match_indices: Vec::new(),
            });
        }
    }

    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.repo.name.len().cmp(&b.repo.name.len()))
            .then_with(|| a.repo.name.to_lowercase().cmp(&b.repo.name.to_lowercase()))
    });
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, path: &str, last_seen: u64) -> Repo {
        Repo {
            name: name.into(),
            path: path.into(),
            vcs: Some("Git".into()),
            sentinels: vec![],
            last_seen,
        }
    }

    #[test]
    fn empty_query_orders_by_recency_then_name() {
        let repos = vec![
            repo("zeta", "/a/zeta", 10),
            repo("alpha", "/a/alpha", 100),
            repo("beta", "/a/beta", 100),
        ];
        let out = search("", &repos, 10);
        let names: Vec<&str> = out.iter().map(|s| s.repo.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn name_match_beats_path_match() {
        let repos = vec![
            repo("unrelated", "/work/api-server/unrelated", 0),
            repo("api", "/work/api", 0),
        ];
        let out = search("api", &repos, 10);
        assert_eq!(out[0].repo.name, "api");
        assert!(!out[0].match_indices.is_empty());
    }

    #[test]
    fn fuzzy_subsequence_matches() {
        let repos = vec![repo("dev-prompt", "/git/dev-prompt", 0)];
        let out = search("dp", &repos, 10);
        assert_eq!(out.len(), 1);
    }
}
