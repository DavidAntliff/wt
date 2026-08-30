//! The five commands. Narration goes to stderr via `info!`; the only stdout
//! writes are the single result path (add / main / resolve, rm's safe dir) and
//! `list`'s table.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::theme::{ColorWhen, Theme};
use crate::worktree::{self, Worktree, branch_label};
use crate::{Result, fail, git, info};

pub struct AddOpts {
    pub branch: Option<String>,
    pub detach: bool,
    pub base: Option<String>,
    pub path: Option<PathBuf>,
    /// Copy the configured [copy] paths into the new worktree: forced on
    /// (`-c`), forced off (`--no-copy`), or `None` = follow the config's
    /// `on-add`.
    pub copy: Option<bool>,
    pub submodules: bool,
    pub no_cd: bool,
}

pub struct RmOpts {
    pub path: String,
    pub force: bool,
    pub delete: bool,
}

#[derive(Default)]
pub struct ListOpts {
    pub porcelain: bool,
    pub absolute: bool,
    pub size: bool,
    pub git: bool,
    pub color: ColorWhen,
}

fn cwd() -> Result<PathBuf> {
    std::env::current_dir().map_err(|e| crate::Error::new(format!("cannot determine cwd: {e}")))
}

/// Prompt on stderr, read a y/yes answer from stdin (EOF -> no).
fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush().ok();
    let mut reply = String::new();
    std::io::stdin().lock().read_line(&mut reply).ok();
    matches!(reply.trim().to_lowercase().as_str(), "y" | "yes")
}

// -----------------------------------------------------------------------------
// add
// -----------------------------------------------------------------------------
pub fn add(opts: &AddOpts) -> Result<()> {
    let cwd = cwd()?;
    git::require_worktree(&cwd)?;

    let main_clone = git::main_clone_of(&cwd)?;
    let container = main_clone.parent().unwrap_or(&main_clone).to_path_buf();
    let base = main_clone
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = base.strip_suffix(".git").unwrap_or(&base).to_string();

    // -d/--detach: no branch at all. The positional is then a COMMIT-ISH
    // (default HEAD), resolved HERE in the current worktree — not in the main
    // clone, which is where the `git worktree add` below actually runs. That is
    // what makes bare `wt add -d` detach at the same commit
    // `git worktree add -d` would.
    let mut detach_sha = String::new();
    let mut detach_short = String::new();
    let name = if opts.detach {
        if opts.base.is_some() {
            fail!(
                "-b/--base makes no sense with -d/--detach; pass the commit-ish as the argument instead"
            );
        }
        let commitish = opts.branch.as_deref().unwrap_or("HEAD");
        detach_sha = git::capture(&["git", "rev-parse", commitish], Some(&cwd))?;
        detach_short = git::capture(&["git", "rev-parse", "--short", &detach_sha], Some(&cwd))?;
        // Name the dir after what the user asked for, or the commit if they
        // gave nothing to name it after.
        opts.branch.clone().unwrap_or_else(|| detach_short.clone())
    } else {
        match &opts.branch {
            Some(branch) => branch.clone(),
            None => fail!("add: BRANCH is required (or use -d/--detach for a branchless worktree)"),
        }
    };

    // Worktree location: -p/--path overrides the default sibling path.
    let wt_path = match &opts.path {
        Some(p) => git::resolve(p),
        None => {
            let norm = worktree::normalize_name(&name);
            if norm.is_empty() {
                fail!("name normalises to empty: '{name}'");
            }
            container.join(format!("wt-{base}-{norm}.git"))
        }
    };

    if wt_path.exists() {
        fail!("target already exists: {}", wt_path.display());
    }

    let main_s = main_clone.to_string_lossy().into_owned();
    let wt_s = wt_path.to_string_lossy().into_owned();

    if opts.detach {
        // Pass the resolved sha, not the commit-ish: this runs in the main
        // clone, where a relative name like HEAD would mean a different commit.
        git::run(
            &[
                "git",
                "-C",
                &main_s,
                "worktree",
                "add",
                "--detach",
                &wt_s,
                &detach_sha,
            ],
            None,
        )?;
        let commitish = opts.branch.as_deref().unwrap_or("HEAD");
        info!("wt: detached at {detach_short} ({commitish}) — no branch");
    } else {
        let branch = &name;
        // Base for a NEW branch: -b/--base if given, else the branch (or
        // commit, if detached) currently checked out in the repo we were
        // invoked from.
        let base_ref = match &opts.base {
            Some(b) => b.clone(),
            None => {
                let head = git::capture(&["git", "rev-parse", "--abbrev-ref", "HEAD"], Some(&cwd))?;
                if head == "HEAD" {
                    // detached HEAD
                    git::capture(&["git", "rev-parse", "HEAD"], Some(&cwd))?
                } else {
                    head
                }
            }
        };

        // Create the worktree. Three cases, in order:
        //   1. branch exists locally        -> check it out
        //   2. branch exists on one remote  -> check it out with tracking
        //                                      (DWIM), unless --base forces a
        //                                      new branch
        //   3. otherwise                    -> create a new branch off base_ref
        let local_exists = git::query(
            &[
                "git",
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
            Some(&main_clone),
        )
        .is_some();
        let remote_ref = if local_exists || opts.base.is_some() {
            None
        } else {
            worktree::remote_branch_match(branch, &main_clone)?
        };
        if local_exists {
            if let Some(b) = &opts.base {
                info!("wt: branch {branch} already exists; ignoring --base {b}");
            }
            git::run(
                &["git", "-C", &main_s, "worktree", "add", &wt_s, branch],
                None,
            )?;
            info!("wt: checked out existing branch {branch}");
        } else if let Some(remote_ref) = remote_ref {
            git::run(
                &[
                    "git",
                    "-C",
                    &main_s,
                    "worktree",
                    "add",
                    "--track",
                    "-b",
                    branch,
                    &wt_s,
                    &remote_ref,
                ],
                None,
            )?;
            info!("wt: created branch {branch} tracking {remote_ref}");
        } else {
            git::run(
                &[
                    "git", "-C", &main_s, "worktree", "add", "-b", branch, &wt_s, &base_ref,
                ],
                None,
            )?;
            info!("wt: created branch {branch} based on {base_ref}");
        }
    }

    // Populate submodules in the new worktree.
    if opts.submodules {
        git::run(
            &[
                "git",
                "-C",
                &wt_s,
                "submodule",
                "update",
                "--init",
                "--recursive",
            ],
            None,
        )?;
    }

    // Optionally seed the new worktree with the configured [copy] paths (the
    // same set `wt copy` syncs): -c forces it, --no-copy suppresses it, and
    // otherwise the config's on-add decides.
    let cfg = crate::config::load();
    for warning in &cfg.warnings {
        info!("wt: {warning}");
    }
    if opts.copy.unwrap_or(cfg.copy.on_add) {
        if cfg.copy.paths.is_empty() {
            info!("wt: nothing configured to copy ([copy] paths is empty); skipped");
        } else {
            copy_paths(&main_clone, &wt_path, &cfg.copy.paths)?;
        }
    }

    if opts.no_cd {
        // Suppress the stdout path so the `wt` shell function does NOT cd; the
        // shell stays in the current worktree.
        info!("wt: worktree ready at {},", wt_path.display());
        info!("    but shell remains in current worktree!");
    } else {
        info!("wt: worktree ready at {}", wt_path.display());
        // stdout = the new worktree path, for the `wt` shell function to cd into.
        println!("{}", wt_path.display());
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// rm
// -----------------------------------------------------------------------------
pub fn rm(opts: &RmOpts) -> Result<()> {
    let mut target = git::resolve(Path::new(&opts.path));
    if !target.is_dir() {
        // Not an existing directory -> treat the arg as a branch/dir query
        // (like `wt cd`) and resolve it to a worktree. Needs to run from inside
        // a worktree of the repo, since we list the repo's worktrees to match.
        let cwd = cwd()?;
        git::require_worktree(&cwd)?;
        target = git::resolve(&worktree::match_worktree(&opts.path, &cwd)?.path);
    }
    let target_s = target.to_string_lossy().into_owned();

    // Must be inside a work tree (nice error for the no-PATH / wrong-dir case).
    if git::query(
        &["git", "-C", &target_s, "rev-parse", "--is-inside-work-tree"],
        None,
    )
    .as_deref()
        != Some("true")
    {
        fail!("not inside a git worktree: {}", target.display());
    }

    // Resolve the worktree root and its owning main clone.
    let top = PathBuf::from(git::capture(
        &["git", "-C", &target_s, "rev-parse", "--show-toplevel"],
        None,
    )?);
    let top_s = top.to_string_lossy().into_owned();
    let main_clone = git::main_clone_of(&target)?;
    let main_s = main_clone.to_string_lossy().into_owned();

    let top_res = git::resolve(&top);
    if top_res == main_clone {
        fail!(
            "refusing to remove the main clone ({})",
            main_clone.display()
        );
    }

    let cwd_res = git::resolve(&cwd()?);
    let removing_cwd = cwd_res == top_res || cwd_res.starts_with(&top_res);
    let status = git::capture(&["git", "-C", &top_s, "status", "--porcelain"], None)?;
    let dirty = !status.is_empty();
    let branch = git::capture(
        &["git", "-C", &top_s, "rev-parse", "--abbrev-ref", "HEAD"],
        None,
    )?;
    let detached = branch == "HEAD";
    let branch_lbl = if detached {
        let short = git::capture(&["git", "-C", &top_s, "rev-parse", "--short", "HEAD"], None)?;
        format!("(detached at {short})")
    } else {
        branch.clone()
    };

    // Always show what is being removed, its branch, and which repo owns it.
    info!("wt: worktree : {}", top.display());
    info!("    branch   : {branch_lbl}");
    info!("    main     : {}", main_clone.display());
    if dirty {
        info!("    uncommitted / untracked changes:");
        for line in status.lines() {
            info!("      {line}");
        }
    }

    // Never throw away dirty changes without -f.
    if dirty && !opts.force {
        fail!(
            "worktree has uncommitted or untracked changes (shown above); pass -f/--force to discard them and remove"
        );
    }

    if opts.force {
        info!("    (--force: removing without confirmation)");
    } else if !confirm("Remove this worktree?") {
        fail!("aborted");
    }

    // --force is always passed: git refuses to remove a worktree containing
    // submodules otherwise. Dirtiness is gated by the check above.
    git::run(
        &[
            "git", "-C", &main_s, "worktree", "remove", "--force", &top_s,
        ],
        None,
    )?;

    if detached {
        info!("wt: removed worktree {} {branch_lbl}", top.display());
    } else {
        info!("wt: removed worktree {} (branch {branch})", top.display());
    }

    // Decide whether to delete the branch from the main clone now that it is no
    // longer checked out:
    //   detached     -> nothing to delete
    //   -d/--delete  -> delete, no prompt
    //   -f/--force   -> keep, no prompt (force defaults to keeping the branch)
    //   otherwise    -> interactively offer to delete it
    let delete_branch = if detached {
        info!("wt: detached — no branch to delete");
        false
    } else if opts.delete {
        true
    } else if opts.force {
        info!(
            "  branch {branch} left intact (delete with -d, or: git -C {main_s} branch -d {branch})"
        );
        false
    } else {
        let del = confirm(&format!(
            "Also delete branch '{branch}' from the main clone?"
        ));
        if !del {
            info!(
                "  branch {branch} left intact (delete later with: git -C {main_s} branch -d {branch})"
            );
        }
        del
    };

    if delete_branch {
        // Not forced (-d, not -D): an unmerged branch is a notice, not a loss.
        if git::run(&["git", "-C", &main_s, "branch", "-d", &branch], None).is_err() {
            info!(
                "wt: branch {branch} NOT deleted (likely unmerged); force with: git -C {main_s} branch -D {branch}"
            );
        } else {
            info!("wt: deleted branch {branch}");
        }
    }

    if removing_cwd {
        info!("  note: this removed the directory your shell was in.");
        // stdout = a safe dir for the `wt` shell function to cd the shell into,
        // so it is not left stranded in the deleted directory.
        println!("{}", main_clone.display());
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// list
// -----------------------------------------------------------------------------

/// Human-readable disk usage of a worktree dir, via `du -sh`.
///
/// Tolerant on purpose: du exits non-zero on unreadable files but still prints
/// the total, so we take whatever it gave us and fall back to '?'.
fn dir_size(path: &Path) -> String {
    let out = Command::new("du").arg("-sh").arg(path).output();
    let size = out
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split('\t')
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    if size.is_empty() {
        "?".to_string()
    } else {
        size
    }
}

/// The four -g/--git column strings for one worktree: STATUS MERGED UPSTREAM LAST.
///
/// Cheap (refs/index only, no tree walk). Tolerant `git::query` throughout: a
/// failing query is a value ('-', '?', 'none'), not a fatal error.
fn git_info(wt: &Worktree, main_head: Option<&str>, is_main: bool) -> [String; 4] {
    let path = wt.path.to_string_lossy().into_owned();

    // STATUS: tracked changes vs untracked files, counted from status --porcelain.
    let status = if wt.bare {
        "-".to_string()
    } else {
        match git::query(&["git", "-C", &path, "status", "--porcelain"], None) {
            None => "?".to_string(),
            Some(out) => {
                let untr = out.lines().filter(|l| l.starts_with("??")).count();
                let modified = out.lines().count() - untr;
                let mut parts = Vec::new();
                if modified > 0 {
                    parts.push(format!("{modified} mod"));
                }
                if untr > 0 {
                    parts.push(format!("{untr} untr"));
                }
                if parts.is_empty() {
                    "clean".to_string()
                } else {
                    parts.join(", ")
                }
            }
        }
    };

    // MERGED: commits on this worktree's HEAD not reachable from the main
    // worktree's HEAD. Shared object store, so run from anywhere.
    let merged = if is_main || wt.bare {
        "-".to_string()
    } else {
        match (main_head, &wt.head) {
            (Some(main_head), Some(head)) => {
                match git::query(
                    &[
                        "git",
                        "-C",
                        &path,
                        "rev-list",
                        "--count",
                        &format!("{main_head}..{head}"),
                    ],
                    None,
                ) {
                    None => "?".to_string(),
                    Some(n) if n == "0" => "merged".to_string(),
                    Some(n) => format!("+{n}"),
                }
            }
            _ => "?".to_string(),
        }
    };

    // UPSTREAM: ahead/behind the branch's tracking ref. No branch -> no upstream.
    let upstream = if wt.bare || wt.branch.is_none() {
        "-".to_string()
    } else {
        match git::query(
            &[
                "git",
                "-C",
                &path,
                "rev-list",
                "--left-right",
                "--count",
                "@{u}...HEAD",
            ],
            None,
        ) {
            None => "none".to_string(),
            Some(out) => {
                let mut it = out.split_whitespace();
                let behind: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let ahead: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let mut parts = Vec::new();
                if ahead > 0 {
                    parts.push(format!("ahead {ahead}"));
                }
                if behind > 0 {
                    parts.push(format!("behind {behind}"));
                }
                if parts.is_empty() {
                    "ok".to_string()
                } else {
                    parts.join(", ")
                }
            }
        }
    };

    // LAST: relative committer date of the worktree's HEAD commit.
    let last = wt
        .head
        .as_ref()
        .and_then(|head| {
            git::query(
                &["git", "-C", &path, "log", "-1", "--format=%cr", head],
                None,
            )
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".to_string());

    [status, merged, upstream, last]
}

/// Format the aligned table (headers + rows-with-markers). Pure, so alignment
/// is unit-testable. Every column is left-aligned except SIZE (numbers read
/// right-aligned).
///
/// Widths come from the PLAIN cell text; the theme paints a cell only after
/// its padding is decided, so escapes never enter a width calculation.
pub fn format_table(
    headers: &[&str],
    rows: &[(Vec<String>, String)],
    theme: Option<&Theme>,
) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            rows.iter()
                .map(|(cells, _)| cells[i].len())
                .chain([h.len()])
                .max()
                .unwrap()
        })
        .collect();
    let fmt = |cells: &[String], paint: &dyn Fn(&str, &str) -> String| -> String {
        cells
            .iter()
            .zip(headers)
            .zip(&widths)
            .map(|((c, h), w)| {
                let pad = " ".repeat(w.saturating_sub(c.len()));
                if *h == "SIZE" {
                    format!("{pad}{}", paint(h, c))
                } else {
                    format!("{}{pad}", paint(h, c))
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    let paint_cell = |h: &str, c: &str| match theme {
        Some(t) => t.paint(h, c),
        None => c.to_string(),
    };
    let paint_header = |_: &str, c: &str| match theme {
        Some(t) => t.paint_header(c),
        None => c.to_string(),
    };
    let mut out = String::new();
    let header_cells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    out.push_str(fmt(&header_cells, &paint_header).trim_end());
    out.push('\n');
    for (cells, marks) in rows {
        // The markers are their own column, so the padding of the columns
        // before them has to survive; only a row's trailing whitespace is
        // stripped.
        out.push_str(format!("{}  {}", fmt(cells, &paint_cell), marks).trim_end());
        out.push('\n');
    }
    out
}

pub fn list(opts: &ListOpts) -> Result<()> {
    let cwd = cwd()?;
    git::require_worktree(&cwd)?;
    let mut trees = worktree::list(&cwd)?;
    let main_clone = git::main_clone_of(&cwd)?;
    let cur_top = git::resolve(&PathBuf::from(git::capture(
        &["git", "rev-parse", "--show-toplevel"],
        Some(&cwd),
    )?));

    let main_head = trees
        .iter()
        .find(|wt| git::resolve(&wt.path) == main_clone)
        .and_then(|wt| wt.head.clone());

    let extra_cells = |wt: &Worktree| -> Vec<String> {
        let mut cells = Vec::new();
        if opts.size {
            cells.push(dir_size(&wt.path));
        }
        if opts.git {
            cells.extend(git_info(
                wt,
                main_head.as_deref(),
                git::resolve(&wt.path) == main_clone,
            ));
        }
        cells
    };

    if opts.porcelain {
        for wt in &trees {
            let mut cells = vec![wt.path.to_string_lossy().into_owned(), branch_label(wt)];
            cells.extend(extra_cells(wt));
            println!("{}", cells.join("\t"));
        }
        return Ok(());
    }

    // Sort: main clone first, then the rest by path. Mark the main clone and cwd.
    trees.sort_by_key(|wt| {
        (
            git::resolve(&wt.path) != main_clone,
            wt.path.to_string_lossy().into_owned(),
        )
    });

    let mut headers = vec!["PATH", "BRANCH"];
    if opts.size {
        headers.push("SIZE");
    }
    if opts.git {
        headers.extend(["STATUS", "MERGED", "UPSTREAM", "LAST"]);
    }

    // The file is read even when colour is off so that a mistake in it is
    // reported; only the resulting styles are discarded.
    let loaded = crate::config::load();
    for warning in &loaded.warnings {
        info!("wt: {warning}");
    }
    let theme = opts.color.enabled().then(|| {
        let mut theme = loaded.theme;
        // anstyle renders whatever it is given, so 24-bit colours have to be
        // brought down to the palette by hand on a terminal without truecolor.
        if !anstyle_query::truecolor() {
            theme.approximate_rgb();
        }
        theme
    });
    let rows: Vec<(Vec<String>, String)> = trees
        .iter()
        .map(|wt| {
            let resolved = git::resolve(&wt.path);
            let mut marks = Vec::new();
            if resolved == main_clone {
                marks.push("main");
            }
            if resolved == cur_top {
                marks.push("cwd");
            }
            let marks: Vec<String> = marks
                .iter()
                .map(|m| match &theme {
                    Some(t) => t.paint_marker(m),
                    None => format!("[{m}]"),
                })
                .collect();
            // Relative to cwd by default (siblings -> ../foo); -a for absolute.
            let disp = if opts.absolute {
                wt.path.to_string_lossy().into_owned()
            } else {
                relpath(&wt.path, &cwd)
            };
            let mut cells = vec![disp, branch_label(wt)];
            cells.extend(extra_cells(wt));
            (cells, marks.join(" "))
        })
        .collect();

    print!("{}", format_table(&headers, &rows, theme.as_ref()));
    Ok(())
}

/// `os.path.relpath`: `path` relative to `base`, using `..` where needed.
/// Both are assumed absolute; falls back to `path` as-is otherwise.
pub fn relpath(path: &Path, base: &Path) -> String {
    let path: Vec<_> = path.components().collect();
    let base: Vec<_> = base.components().collect();
    let common = path
        .iter()
        .zip(&base)
        .take_while(|(a, b)| **a == **b)
        .count();
    let mut parts: Vec<String> =
        std::iter::repeat_n("..".to_string(), base.len() - common).collect();
    parts.extend(
        path[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

// -----------------------------------------------------------------------------
// main
// -----------------------------------------------------------------------------
pub fn main_cmd() -> Result<()> {
    let cwd = cwd()?;
    git::require_worktree(&cwd)?;
    let main_clone = git::main_clone_of(&cwd)?;
    info!("wt: main {}", main_clone.display());
    // stdout = the main-clone root, for the `wt` shell function to cd into.
    println!("{}", main_clone.display());
    Ok(())
}

// -----------------------------------------------------------------------------
// copy
// -----------------------------------------------------------------------------

/// Recursive copy (Python's `shutil.copytree`); merges into an existing dst.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Copy the configured paths from the main clone into `dst_root`. Shared by
/// `wt copy` and `add`'s copy-on-add. A path missing in the main clone is a
/// notice, not an error; destinations are assumed clear (the callers gate
/// that).
fn copy_paths(main_clone: &Path, dst_root: &Path, paths: &[String]) -> Result<()> {
    for rel in paths {
        let src = main_clone.join(rel);
        let dst = dst_root.join(rel);
        if src.is_dir() {
            copy_dir_all(&src, &dst)
        } else if src.is_file() {
            // Nested entries like .vscode/settings.json need their parent.
            dst.parent()
                .map(std::fs::create_dir_all)
                .transpose()
                .and_then(|_| std::fs::copy(&src, &dst).map(|_| ()))
        } else {
            info!("wt: no {rel} in {}; skipped", main_clone.display());
            continue;
        }
        .map_err(|e| crate::Error::new(format!("copying {rel}: {e}")))?;
        info!("wt: copied {rel} from {}", main_clone.display());
    }
    Ok(())
}

pub fn copy(force: bool) -> Result<()> {
    let cwd = cwd()?;
    git::require_worktree(&cwd)?;
    let main_clone = git::main_clone_of(&cwd)?;
    let cur_top = git::resolve(&PathBuf::from(git::capture(
        &["git", "rev-parse", "--show-toplevel"],
        Some(&cwd),
    )?));

    if cur_top == main_clone {
        fail!(
            "current worktree is the main clone ({}); `wt copy` copies FROM the main clone \
             INTO a worktree — run it from inside a worktree",
            main_clone.display()
        );
    }

    let loaded = crate::config::load();
    for warning in &loaded.warnings {
        info!("wt: {warning}");
    }
    let paths = loaded.copy.paths;
    if paths.is_empty() {
        fail!(
            "nothing configured to copy; list paths under [copy] in the config file \
             (wt --generate-config shows the format)"
        );
    }

    // Check ALL destinations before touching anything, so a run without -f is
    // all-or-nothing.
    let existing: Vec<&String> = paths
        .iter()
        .filter(|rel| cur_top.join(rel).exists())
        .collect();
    if !existing.is_empty() {
        if !force {
            fail!(
                "already in this worktree: {} — pass -f/--force to overwrite",
                existing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        // A true overwrite, not a merge: remove each existing destination first.
        for rel in existing {
            let dst = cur_top.join(rel);
            let removed = if dst.is_dir() {
                std::fs::remove_dir_all(&dst)
            } else {
                std::fs::remove_file(&dst)
            };
            removed.map_err(|e| crate::Error::new(format!("removing {}: {e}", dst.display())))?;
            info!("wt: removed existing {rel} in {}", cur_top.display());
        }
    }

    copy_paths(&main_clone, &cur_top, &paths)?;
    info!(
        "wt: copied from {} -> {}",
        main_clone.display(),
        cur_top.display()
    );
    Ok(())
}

// -----------------------------------------------------------------------------
// resolve
// -----------------------------------------------------------------------------
pub fn resolve(query: &str) -> Result<()> {
    let cwd = cwd()?;
    git::require_worktree(&cwd)?;
    let wt = worktree::match_worktree(query, &cwd)?;
    info!("wt: {}  ->  {}", branch_label(&wt), wt.path.display());
    // stdout = the matched worktree path (the `wt cd` shell command cds into it).
    println!("{}", wt.path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relpath_basics() {
        assert_eq!(relpath(Path::new("/a/b/c"), Path::new("/a/b")), "c");
        assert_eq!(relpath(Path::new("/a/x"), Path::new("/a/b/c")), "../../x");
        assert_eq!(relpath(Path::new("/a/b"), Path::new("/a/b")), ".");
    }

    #[test]
    fn table_alignment_and_size_column() {
        let headers = ["PATH", "BRANCH", "SIZE"];
        let rows = vec![
            (
                vec!["../repo".into(), "main".into(), "1.2G".into()],
                "[main] [cwd]".to_string(),
            ),
            (
                vec![
                    "../wt-repo-foo.git".into(),
                    "dev/foo".into(),
                    "80M".into(),
                ],
                String::new(),
            ),
        ];
        let out = format_table(&headers, &rows, None);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "PATH                BRANCH     SIZE");
        // SIZE right-aligned, markers appended, no trailing whitespace.
        assert_eq!(
            lines[1],
            "../repo             main       1.2G  [main] [cwd]"
        );
        assert_eq!(lines[2], "../wt-repo-foo.git  dev/foo   80M");
        assert!(out.lines().all(|l| l == l.trim_end()));
    }

    /// Strip ANSI SGR sequences (ESC ... 'm') without a regex dependency.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn alignment_is_unaffected_by_colour() {
        // Escapes must never enter a width calculation: stripping them from the
        // painted table must reproduce the plain table exactly.
        let headers = [
            "PATH", "BRANCH", "SIZE", "STATUS", "MERGED", "UPSTREAM", "LAST",
        ];
        let rows = vec![
            (
                vec![
                    "../repo".into(),
                    "main".into(),
                    "12G".into(),
                    "clean".into(),
                    "-".into(),
                    "ok".into(),
                    "2 days ago".into(),
                ],
                "[main]".to_string(),
            ),
            (
                vec![
                    "../wt-repo-foo.git".into(),
                    "dev/foo".into(),
                    "1.2G".into(),
                    "2 mod, 1 untr".into(),
                    "+3".into(),
                    "none".into(),
                    "3 weeks ago".into(),
                ],
                String::new(),
            ),
        ];
        let plain = format_table(&headers, &rows, None);
        let painted = format_table(&headers, &rows, Some(&crate::config::defaults()));
        assert_ne!(plain, painted);
        assert_eq!(strip_ansi(&painted), plain);
    }
}
