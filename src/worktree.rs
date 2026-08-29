//! Worktree parsing, labelling, and query matching.

use std::path::{Path, PathBuf};

use crate::{Error, Result, fail, git, info};

#[derive(Debug, Default, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
    /// Abbreviated commit for a detached worktree's list label.
    pub short: Option<String>,
}

/// Parse `git worktree list --porcelain` output. Pure — no git calls, no
/// `short` abbreviation (that needs the repo; see [`list`]).
pub fn parse_porcelain(out: &str) -> Vec<Worktree> {
    let mut trees: Vec<Worktree> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            trees.push(Worktree {
                path: PathBuf::from(path),
                ..Default::default()
            });
            continue;
        }
        let Some(cur) = trees.last_mut() else {
            continue;
        };
        if let Some(head) = line.strip_prefix("HEAD ") {
            cur.head = Some(head.to_string());
        } else if let Some(refname) = line.strip_prefix("branch ") {
            cur.branch = Some(
                refname
                    .strip_prefix("refs/heads/")
                    .unwrap_or(refname)
                    .to_string(),
            );
        } else if line == "bare" {
            cur.bare = true;
        } else if line == "detached" {
            cur.detached = true;
        } else if line.starts_with("locked") {
            cur.locked = true;
        } else if line.starts_with("prunable") {
            cur.prunable = true;
        }
    }
    trees
}

/// All worktrees of the repo containing `cwd`.
pub fn list(cwd: &Path) -> Result<Vec<Worktree>> {
    let out = git::capture(&["git", "worktree", "list", "--porcelain"], Some(cwd))?;
    let mut trees = parse_porcelain(&out);
    // Abbreviate the commit of every detached worktree, for its list label. Via
    // `rev-parse --short` rather than a fixed slice, so it honours core.abbrev
    // and grows as far as this repo needs for uniqueness.
    for wt in &mut trees {
        if wt.detached
            && let Some(head) = &wt.head
        {
            wt.short = Some(git::capture(
                &["git", "rev-parse", "--short", head],
                Some(cwd),
            )?);
        }
    }
    Ok(trees)
}

pub fn branch_label(wt: &Worktree) -> String {
    if wt.bare {
        return "(bare)".to_string();
    }
    if let Some(branch) = &wt.branch {
        return branch.clone();
    }
    if wt.detached {
        return match &wt.short {
            Some(short) => format!("(detached at {short})"),
            None => "(detached)".to_string(),
        };
    }
    "(unknown)".to_string()
}

/// Normalise a name for the worktree dir: any run of chars outside
/// [A-Za-z0-9._-] becomes a single '-', leading/trailing '-' stripped.
pub fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The names a query is matched against: the worktree dir basename, and the
/// branch if there is one (so detached worktrees are still reachable by path).
fn keys(wt: &Worktree) -> Vec<String> {
    let mut ks = vec![
        wt.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ];
    if let Some(branch) = &wt.branch {
        ks.push(branch.clone());
    }
    ks
}

/// Pure matching core: prefer exact matches (branch or dir), else
/// case-insensitive substring matches on either.
pub fn select<'a>(query: &str, trees: &'a [Worktree]) -> Vec<&'a Worktree> {
    let exact: Vec<&Worktree> = trees
        .iter()
        .filter(|wt| keys(wt).iter().any(|k| k == query))
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    let q = query.to_lowercase();
    trees
        .iter()
        .filter(|wt| keys(wt).iter().any(|k| k.to_lowercase().contains(&q)))
        .collect()
}

/// Map QUERY to a single worktree by branch name OR dir basename. Narrates and
/// exits like `resolve`: ambiguous -> list candidates on stderr, exit 2;
/// none -> error on stderr (with a `wt add` hint), exit 1. Shared by `resolve`
/// and by `rm`'s branch-name lookup.
pub fn match_worktree(query: &str, cwd: &Path) -> Result<Worktree> {
    let trees = list(cwd)?;
    let matches = select(query, &trees);
    match matches.len() {
        0 => fail!(
            "no worktree matching '{query}' (by branch or path) (create one with: wt add {query})"
        ),
        1 => Ok(matches[0].clone()),
        n => {
            info!("wt: {n} worktrees match '{query}' — be more specific:");
            for wt in matches {
                info!("    {}  ->  {}", branch_label(wt), wt.path.display());
            }
            Err(Error::silent(2))
        }
    }
}

/// If exactly one remote has a branch named `branch`, return its tracking-ref
/// short name (e.g. 'origin/dev/foo'); else None.
///
/// Mirrors `git worktree add`'s own DWIM so that `wt add <name>` for a branch
/// that exists only on a remote (a teammate's branch, or one you pushed
/// earlier) checks it out with tracking, instead of silently forking a NEW
/// branch off the current HEAD. Reflects the last fetch — fetch first if the
/// remote branch is newer.
pub fn remote_branch_match(branch: &str, cwd: &Path) -> Result<Option<String>> {
    let out = git::capture(
        &[
            "git",
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/remotes/",
        ],
        Some(cwd),
    )?;
    // Short name is "<remote>/<rest>"; we want rest == branch (branch may
    // itself contain slashes, e.g. dev/foo).
    let matches: Vec<&str> = out
        .lines()
        .filter(|r| r.split_once('/').is_some_and(|(_, rest)| rest == branch))
        .collect();
    Ok(if matches.len() == 1 {
        Some(matches[0].to_string())
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt(path: &str, branch: Option<&str>) -> Worktree {
        Worktree {
            path: PathBuf::from(path),
            branch: branch.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn porcelain_parses_main_linked_detached_bare() {
        let out = "worktree /repo\nHEAD 1111111111111111111111111111111111111111\nbranch refs/heads/main\n\n\
                   worktree /wt-repo-foo.git\nHEAD 2222222222222222222222222222222222222222\nbranch refs/heads/dev/foo\n\n\
                   worktree /wt-repo-det.git\nHEAD 3333333333333333333333333333333333333333\ndetached\n\n\
                   worktree /bare\nbare\nlocked reason\nprunable gone\n";
        let trees = parse_porcelain(out);
        assert_eq!(trees.len(), 4);
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert_eq!(trees[1].branch.as_deref(), Some("dev/foo"));
        assert!(trees[2].detached && trees[2].branch.is_none());
        assert!(trees[3].bare && trees[3].locked && trees[3].prunable);
    }

    #[test]
    fn labels() {
        assert_eq!(branch_label(&wt("/a", Some("main"))), "main");
        assert_eq!(
            branch_label(&Worktree {
                bare: true,
                ..Default::default()
            }),
            "(bare)"
        );
        let det = Worktree {
            detached: true,
            short: Some("abc123".into()),
            ..Default::default()
        };
        assert_eq!(branch_label(&det), "(detached at abc123)");
        assert_eq!(branch_label(&Worktree::default()), "(unknown)");
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize_name("dev/fix-foo"), "dev-fix-foo");
        assert_eq!(normalize_name("a//b??c"), "a-b-c");
        assert_eq!(normalize_name("--x--"), "x");
        assert_eq!(normalize_name("v1.2.3_rc"), "v1.2.3_rc");
        assert_eq!(normalize_name("///"), "");
    }

    #[test]
    fn select_prefers_exact_over_substring() {
        let trees = vec![
            wt("/wt-r-foo.git", Some("foo")),
            wt("/wt-r-foobar.git", Some("foobar")),
        ];
        let m = select("foo", &trees);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].branch.as_deref(), Some("foo"));
    }

    #[test]
    fn select_substring_is_case_insensitive_and_can_be_ambiguous() {
        let trees = vec![
            wt("/wt-r-foo.git", Some("dev/FOO")),
            wt("/wt-r-food.git", Some("food")),
        ];
        assert_eq!(select("foo", &trees).len(), 2);
        assert_eq!(select("FOOD", &trees).len(), 1);
    }

    #[test]
    fn select_matches_dir_basename_for_detached() {
        let trees = vec![wt("/wt-r-v1.2.3.git", None)];
        assert_eq!(select("v1.2", &trees).len(), 1);
        assert!(select("nope", &trees).is_empty());
    }
}
