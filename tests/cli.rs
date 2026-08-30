//! End-to-end tests against the real `wt` binary and real git, inside
//! temporary directories. No test touches any real repository: every command
//! runs with its cwd inside a tempdir created by `setup()`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

/// A container dir holding a main clone at `<dir>/repo`, removed on drop.
struct Temp {
    dir: PathBuf,
}

impl Temp {
    fn repo(&self) -> PathBuf {
        self.dir.join("repo")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A fresh container with an initialised main clone (one commit, branch main).
fn setup() -> Temp {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wt-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    Temp { dir }
}

fn wt(cwd: &Path, args: &[&str]) -> Output {
    // Point WT_CONFIG at a path that never exists so tests see the built-in
    // defaults, not the developer's real ~/.config/wt/config.toml.
    wt_with_config(cwd, Path::new("/nonexistent/wt-test-config.toml"), args)
}

fn wt_cmd(cwd: &Path, config: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wt"));
    cmd.args(args)
        .current_dir(cwd)
        .env("WT_CONFIG", config)
        // Modern git blocks file-protocol submodules by default; allow them
        // for every git wt spawns so submodule tests can clone local fixtures.
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        // Colour assertions must not depend on the developer's environment.
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE");
    cmd
}

fn wt_with_config(cwd: &Path, config: &Path, args: &[&str]) -> Output {
    wt_cmd(cwd, config, args).output().expect("run wt")
}

/// Run wt with `input` piped to stdin (for the interactive prompts).
fn wt_stdin(cwd: &Path, args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = wt_cmd(cwd, Path::new("/nonexistent/wt-test-config.toml"), args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn wt");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().expect("run wt")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn add_creates_sibling_worktree_and_prints_only_the_path() {
    let t = setup();
    let out = wt(&t.repo(), &["add", "dev/fix-foo"]);
    assert!(out.status.success(), "{}", stderr(&out));

    // stdout is exactly one line: the new worktree path.
    let so = stdout(&out);
    let lines: Vec<&str> = so.lines().collect();
    assert_eq!(lines.len(), 1, "stdout must be a single path, got: {so:?}");
    let path = PathBuf::from(lines[0]);
    assert!(path.is_dir());
    assert_eq!(path.file_name().unwrap(), "wt-repo-dev-fix-foo.git");
    assert_eq!(path.parent().unwrap(), t.dir.canonicalize().unwrap());

    // Narration went to stderr, and the branch was created.
    assert!(stderr(&out).contains("created branch dev/fix-foo"));
    let head = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        "dev/fix-foo"
    );
}

#[test]
fn add_existing_branch_checks_it_out() {
    let t = setup();
    git(&t.repo(), &["branch", "existing"]);
    let out = wt(&t.repo(), &["add", "existing"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("checked out existing branch existing"));
}

#[test]
fn add_detached_names_dir_after_commit_and_has_no_branch() {
    let t = setup();
    let out = wt(&t.repo(), &["add", "-d"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("no branch"));
    let path = PathBuf::from(stdout(&out).trim());
    let head = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&path)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        "HEAD",
        "worktree must be detached"
    );
}

#[test]
fn add_detach_rejects_base() {
    let t = setup();
    let out = wt(&t.repo(), &["add", "-d", "-b", "main"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("makes no sense"));
}

#[test]
fn add_no_cd_prints_nothing_on_stdout() {
    let t = setup();
    let out = wt(&t.repo(), &["add", "--no-cd", "quiet"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert!(stderr(&out).contains("shell remains in current worktree"));
}

#[test]
fn add_without_branch_fails() {
    let t = setup();
    let out = wt(&t.repo(), &["add"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("BRANCH is required"));
}

#[test]
fn add_refuses_existing_target() {
    let t = setup();
    assert!(wt(&t.repo(), &["add", "dup"]).status.success());
    let out = wt(&t.repo(), &["add", "dup"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("target already exists"));
}

#[test]
fn resolve_exact_substring_ambiguous_none() {
    let t = setup();
    assert!(wt(&t.repo(), &["add", "feature/one"]).status.success());
    assert!(wt(&t.repo(), &["add", "feature/two"]).status.success());

    // Substring, unique.
    let out = wt(&t.repo(), &["resolve", "one"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().ends_with("wt-repo-feature-one.git"));

    // Exact beats substring: branch "feature/one" is exact even though
    // "feature" would be ambiguous.
    let out = wt(&t.repo(), &["resolve", "feature/one"]);
    assert!(out.status.success());

    // Ambiguous -> exit 2, candidates on stderr, nothing on stdout.
    let out = wt(&t.repo(), &["resolve", "feature"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert!(stderr(&out).contains("be more specific"));

    // No match -> exit 1 with a `wt add` hint.
    let out = wt(&t.repo(), &["resolve", "zzz"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("wt add zzz"));
}

#[test]
fn main_prints_the_main_clone_from_a_worktree() {
    let t = setup();
    let add = wt(&t.repo(), &["add", "elsewhere"]);
    let wt_path = PathBuf::from(stdout(&add).trim());
    let out = wt(&wt_path, &["main"]);
    assert!(out.status.success());
    assert_eq!(
        PathBuf::from(stdout(&out).trim()),
        t.repo().canonicalize().unwrap()
    );
}

#[test]
fn rm_refuses_the_main_clone() {
    let t = setup();
    let out = wt(&t.repo(), &["rm", "-f"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("refusing to remove the main clone"));
}

#[test]
fn rm_force_from_inside_removes_and_prints_main_clone() {
    let t = setup();
    let add = wt(&t.repo(), &["add", "doomed"]);
    let wt_path = PathBuf::from(stdout(&add).trim());

    let out = wt(&wt_path, &["rm", "-f"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // Removed the cwd -> the safe dir (main clone) is printed for the shell.
    assert_eq!(
        PathBuf::from(stdout(&out).trim()),
        t.repo().canonicalize().unwrap()
    );
    assert!(!wt_path.exists());
    // -f without -d keeps the branch.
    assert!(stderr(&out).contains("left intact"));
}

#[test]
fn rm_by_query_with_branch_delete() {
    let t = setup();
    let add = wt(&t.repo(), &["add", "byquery"]);
    let wt_path = PathBuf::from(stdout(&add).trim());

    // Branch is unmerged from main's perspective? No — no new commits, so -d works.
    let out = wt(&t.repo(), &["rm", "-f", "-d", "byquery"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // cwd (the main clone) was not inside the removed tree -> no stdout path.
    assert_eq!(stdout(&out), "");
    assert!(!wt_path.exists());
    assert!(stderr(&out).contains("deleted branch byquery"));
}

#[test]
fn rm_dirty_requires_force() {
    let t = setup();
    let add = wt(&t.repo(), &["add", "dirty"]);
    let wt_path = PathBuf::from(stdout(&add).trim());
    std::fs::write(wt_path.join("untracked.txt"), "x\n").unwrap();

    let out = wt(&t.repo(), &["rm", "dirty"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("uncommitted or untracked"));
    assert!(wt_path.exists());
}

#[test]
fn list_porcelain_and_table() {
    let t = setup();
    assert!(wt(&t.repo(), &["add", "listed"]).status.success());

    let out = wt(&t.repo(), &["list", "-p"]);
    assert!(out.status.success());
    let so = stdout(&out);
    let lines: Vec<&str> = so.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let (path, branch) = line.split_once('\t').expect("tab-separated");
        assert!(Path::new(path).is_absolute());
        assert!(!branch.is_empty());
    }

    // Bare `wt` -> the table, main clone first with markers.
    let out = wt(&t.repo(), &[]);
    assert!(out.status.success());
    let so = stdout(&out);
    let lines: Vec<&str> = so.lines().collect();
    assert!(lines[0].starts_with("PATH"));
    assert!(lines[1].contains("[main] [cwd]"), "got: {}", lines[1]);
    assert!(lines[2].contains("listed"));
}

#[test]
fn leading_flag_means_list() {
    let t = setup();
    let out = wt(&t.repo(), &["-g"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let so = stdout(&out);
    assert!(so.lines().next().unwrap().contains("STATUS"));
    assert!(so.contains("clean"));
}

/// Commit a file-protocol submodule at `sub/` into the main clone.
fn add_submodule(t: &Temp) {
    let src = t.dir.join("subsrc");
    std::fs::create_dir_all(&src).unwrap();
    git(&src, &["init", "-b", "main"]);
    std::fs::write(src.join("s.txt"), "s\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-m", "sub"]);
    git(
        &t.repo(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            src.to_str().unwrap(),
            "sub",
        ],
    );
    git(&t.repo(), &["commit", "-m", "add submodule"]);
}

#[test]
fn add_skips_submodules_by_default_with_flag_and_config_overrides() {
    let t = setup();
    add_submodule(&t);

    // Default: NOT populated, notice on stderr.
    let out = wt(&t.repo(), &["add", "one"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let p = PathBuf::from(stdout(&out).trim());
    assert!(p.join(".gitmodules").is_file());
    assert!(!p.join("sub/s.txt").exists());
    assert!(stderr(&out).contains("submodules present but not checked out"));

    // -s/--submodules forces the checkout.
    let out = wt(&t.repo(), &["add", "-s", "two"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let p = PathBuf::from(stdout(&out).trim());
    assert!(p.join("sub/s.txt").is_file());
    assert!(!stderr(&out).contains("not checked out"));

    // [submodules] on-add = true populates; --no-submodules still wins.
    let cfg = t.dir.join("sub-config.toml");
    std::fs::write(&cfg, "[submodules]\non-add = true\n").unwrap();
    let out = wt_with_config(&t.repo(), &cfg, &["add", "three"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        PathBuf::from(stdout(&out).trim())
            .join("sub/s.txt")
            .is_file()
    );
    let out = wt_with_config(
        &t.repo(),
        &cfg,
        &["add", "--no-submodules", "--color", "always", "four"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !PathBuf::from(stdout(&out).trim())
            .join("sub/s.txt")
            .exists()
    );
    // The skipped-submodules warning is bright yellow.
    assert!(
        stderr(&out).contains("\u{1b}[93mwt: submodules present but not checked out"),
        "{:?}",
        stderr(&out)
    );

    // The two flags conflict (clap usage error, exit 2).
    let out = wt(&t.repo(), &["add", "--submodules", "--no-submodules", "x"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn narration_colour_honours_the_global_color_option() {
    let t = setup();

    // --color always: info lines cyan, "worktree ready" bright cyan; stdout
    // stays a bare unpainted path.
    let out = wt(&t.repo(), &["add", "--color", "always", "painted"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let se = stderr(&out);
    assert!(se.contains("\u{1b}[36mwt: created branch"), "{se:?}");
    assert!(se.contains("\u{1b}[96mwt: worktree ready"), "{se:?}");
    assert!(!stdout(&out).contains('\u{1b}'));

    // Errors are bright red.
    let out = wt(&t.repo(), &["add", "--color", "always", "painted"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("\u{1b}[91mwt: target already exists"),
        "{:?}",
        stderr(&out)
    );

    // Auto (default): stderr is not a terminal here, so no escapes at all.
    let out = wt(&t.repo(), &["add", "plain"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stderr(&out).contains('\u{1b}'));

    // Interactive prompts are bright magenta ("n" aborts, exit 1).
    let out = wt_stdin(
        &t.repo().parent().unwrap().join("wt-repo-plain.git"),
        &["rm", "--color", "always"],
        "n\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("\u{1b}[95mRemove this worktree? [y/N]"),
        "{:?}",
        stderr(&out)
    );
}

#[test]
fn add_without_submodules_prints_no_submodule_notice() {
    let t = setup();
    let out = wt(&t.repo(), &["add", "plain"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stderr(&out).contains("submodules"));
}

/// A config file whose [copy] section lists `.idea` and a nested file.
fn copy_config(t: &Temp, on_add: bool) -> PathBuf {
    let cfg = t.dir.join("copy-config.toml");
    std::fs::write(
        &cfg,
        format!("[copy]\non-add = {on_add}\npaths = [\".idea\", \".vscode/settings.json\"]\n"),
    )
    .unwrap();
    cfg
}

/// Seed the main clone with the sources `copy_config` refers to.
fn seed_sources(t: &Temp) {
    let idea = t.repo().join(".idea");
    std::fs::create_dir_all(idea.join("sub")).unwrap();
    std::fs::write(idea.join("workspace.xml"), "<xml/>\n").unwrap();
    std::fs::write(idea.join("sub").join("deep.xml"), "<deep/>\n").unwrap();
    std::fs::create_dir_all(t.repo().join(".vscode")).unwrap();
    std::fs::write(t.repo().join(".vscode/settings.json"), "{}\n").unwrap();
}

#[test]
fn copy_syncs_configured_paths_into_worktree() {
    let t = setup();
    let cfg = copy_config(&t, false);
    seed_sources(&t);

    let add = wt(&t.repo(), &["add", "target"]);
    let wt_path = PathBuf::from(stdout(&add).trim());
    // on-add is false, so the fresh worktree was NOT seeded.
    assert!(!wt_path.join(".idea").exists());

    // From the main clone: refused (it is the source, not a target).
    let out = wt_with_config(&t.repo(), &cfg, &["copy"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("main clone"));

    // From the worktree: dirs copied recursively, nested file too, no stdout.
    let out = wt_with_config(&wt_path, &cfg, &["copy"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        std::fs::read_to_string(wt_path.join(".idea/sub/deep.xml")).unwrap(),
        "<deep/>\n"
    );
    assert!(wt_path.join(".vscode/settings.json").exists());

    // Existing destinations need -f; -f replaces (old contents gone, not merged).
    let out = wt_with_config(&wt_path, &cfg, &["copy"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--force"));
    std::fs::write(wt_path.join(".idea/stale.xml"), "old\n").unwrap();
    let out = wt_with_config(&wt_path, &cfg, &["copy", "-f"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!wt_path.join(".idea/stale.xml").exists());
    assert!(wt_path.join(".idea/workspace.xml").exists());
}

#[test]
fn copy_with_nothing_configured_is_an_error_and_missing_source_a_notice() {
    let t = setup();
    let add = wt(&t.repo(), &["add", "empty"]);
    let wt_path = PathBuf::from(stdout(&add).trim());

    // Default config: no paths -> error pointing at the config.
    let out = wt(&wt_path, &["copy"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("nothing configured to copy"));

    // Configured but absent in the main clone -> notice, success.
    let cfg = copy_config(&t, false);
    let out = wt_with_config(&wt_path, &cfg, &["copy"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("skipped"));
}

#[test]
fn add_copy_on_add_and_overrides() {
    let t = setup();
    seed_sources(&t);

    // on-add = true seeds automatically.
    let cfg = copy_config(&t, true);
    let out = wt_with_config(&t.repo(), &cfg, &["add", "auto"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let so = stdout(&out);
    assert_eq!(so.lines().count(), 1, "stdout stays a single path: {so:?}");
    assert!(
        PathBuf::from(so.trim())
            .join(".idea/workspace.xml")
            .exists()
    );

    // --no-copy suppresses it.
    let out = wt_with_config(&t.repo(), &cfg, &["add", "--no-copy", "manual"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!PathBuf::from(stdout(&out).trim()).join(".idea").exists());

    // -c forces it when on-add = false.
    let cfg_off = copy_config(&t, false);
    let out = wt_with_config(&t.repo(), &cfg_off, &["add", "-c", "forced"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(PathBuf::from(stdout(&out).trim()).join(".idea").exists());

    // -c with nothing configured: a notice, not an error.
    let out = wt(&t.repo(), &["add", "-c", "noconf"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stderr(&out).contains("nothing configured to copy"));

    // -c and --no-copy conflict.
    let out = wt(&t.repo(), &["add", "-c", "--no-copy", "clash"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn generate_config_prints_the_template_anywhere() {
    let t = setup();
    // Works even outside a repo, and is exempt from the leading-flag->list rewrite.
    let out = wt(&t.dir, &["--generate-config"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let so = stdout(&out);
    assert!(so.starts_with("# wt colour configuration."));
    assert!(so.contains("[colour]"));
}

#[test]
fn wt_config_env_overrides_a_colour_and_bad_keys_only_warn() {
    let t = setup();
    let cfg = t.dir.join("cfg.toml");
    std::fs::write(&cfg, "[colour]\nheader = \"red\"\nbogus = \"blue\"\n").unwrap();

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_wt"))
            .args(args)
            .current_dir(t.repo())
            .env("WT_CONFIG", &cfg)
            .output()
            .expect("run wt")
    };

    let out = run(&["list", "--color=always"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // header now red (31), not the default bold cyan.
    assert!(
        stdout(&out).starts_with("\x1b[31mPATH"),
        "got: {:?}",
        stdout(&out)
    );
    // The unknown key is a warning on stderr, never a failure.
    assert!(stderr(&out).contains("bogus"));

    // With colour off the table is plain, but the mistake is still reported.
    let out = run(&["list", "--color=never"]);
    assert!(stdout(&out).starts_with("PATH"));
    assert!(stderr(&out).contains("bogus"));
}

#[test]
fn outside_a_repo_is_an_error() {
    let t = setup();
    // t.dir itself is the container, not a repo.
    let out = wt(&t.dir, &["list"]);
    assert_eq!(out.status.code(), Some(1));
    let out = wt(&t.dir, &["main"]);
    assert_eq!(out.status.code(), Some(1));
}
