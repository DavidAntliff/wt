# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`wt` is a git-worktree front end: a Rust binary (`add` / `rm` / `list` / `main` /
`resolve` / `idea`) plus a sourced bash function (`wt-shell`) that wraps it and performs
the `cd`s a child process cannot. `README.md` is the user-facing guide; `SPEC.md` is the
authoritative behaviour spec — keep it in sync with any behaviour change.

## Hard constraints

**The stdout/stderr contract is THE interface — the shell function cds into whatever
lands on stdout.** For `add`, `main`, `resolve`, and `rm`, stdout may carry at most a
single bare path; every human-facing message (progress, prompts, errors) goes to
stderr. `list` is the one command whose stdout is its output. Child git commands run
through `git::run`, which redirects their stdout to our stderr — never `Stdio::inherit()`
a git command's stdout. A stray `println!` here is not cosmetic; it makes `wt` cd
somewhere absurd.

**Exit codes are load-bearing:** 1 for errors, 2 for an ambiguous `resolve`/`rm` query
(the ambiguous path narrates its candidates itself and uses `Error::silent(2)`). clap
usage errors exit 2, matching the old argparse behaviour.

**Tests must never touch a real repository.** Every integration test runs inside a
tempdir built by `setup()` in `tests/cli.rs`; keep new tests on that helper. Unit tests
are pure — porcelain parsing, matching, normalisation and table formatting all take
data, not a cwd, precisely so they need no git.

**`rm` must never discard uncommitted work without `-f`,** and it must always refuse
the main clone. `git worktree remove --force` is always passed (git refuses on
submodule-containing worktrees otherwise); the dirtiness gate is ours, above it.

## Commands

```sh
cargo build
cargo test                    # unit + integration; the gate
cargo test --lib              # unit tests only
cargo test --test cli         # integration tests (real git in tempdirs)
cargo clippy --all-targets    # expected to be warning-free
cargo fmt
just install                  # release build -> ~/.local/bin (binary + wt-shell)
```

Manual smoke testing: make a throwaway repo, put `target/debug` on PATH, and
`source ./wt-shell` in that shell. Check stdout purity with e.g.
`wt add x >/dev/null` — all narration must still appear.

## Architecture

`src/main.rs` is a thin clap wrapper; everything testable lives in the library.
The leading-flag rewrite (`wt -sg` → `wt list -sg`) and the bare-`wt`-means-`list`
default live in `main.rs`, before clap parses.

- `git.rs` — subprocess helpers. `capture` (result on stdout), `run` (child stdout →
  our stderr), `query` (tolerant: `Option`, for `list -g` where a failed query is a
  cell value like `?`/`none`, never an abort), `main_clone_of`, `resolve` (canonicalize
  with a non-strict fallback, because worktree paths are compared under symlinked
  tempdirs but `add`'s target does not exist yet).
- `worktree.rs` — porcelain parsing (`parse_porcelain` is pure), branch labels, name
  normalisation, and the query matcher. `select` (pure: exact beats substring, both
  matched against branch name AND dir basename so detached worktrees are reachable)
  is wrapped by `match_worktree`, which is shared by `resolve` and `rm` — matching
  behaviour must stay identical between them.
- `commands.rs` — the commands, plus `list`'s helpers (`git_info`, `dir_size`,
  `format_table`, `relpath`).

`wt-shell` is bash, sourced, and dispatches on the first word; its dispatch table is in
SPEC.md. It finds the binary as `command wt` (the function shadows it), so nothing is
path-configured. Any new binary command that prints a result path needs a matching
intercept branch there — and its help flags must be detected and forwarded BEFORE the
capture, or `--help` output gets cd'd into.

## Things that look odd and are not

- **`add -d` resolves the commit-ish in the CURRENT worktree, then passes the sha** to
  a `git worktree add` running in the main clone — `HEAD` means different commits in
  the two places. Passing the name through would silently detach at the wrong commit.
- **`wt cd main` is special-cased in wt-shell** to `wt main` rather than left to
  `resolve`, so it deterministically lands on the main clone instead of depending on a
  branch literally named `main` (and instead of fuzzy-matching `wt-…-maintenance.git`).
- **`parent` is a hidden alias for `main`** (the command's former name). Keep it
  working, keep it out of help output.
- **`rm` prints a path on stdout only when it removed the directory the shell was
  standing in** — that is the shell function's cue to move the shell to safety, not a
  general result.
- **`wt idea` is a stopgap name.** The plan is to generalise it into `wt copy`, taking
  the list of directories to sync from an environment variable instead of hardcoding
  `.idea/`. Keep that in mind before deepening its `.idea`-specific behaviour.

## Provenance

Port of a personal Python + bash implementation (`wt.py` + `wt-shell`). Public repo,
MIT. Repo-local git identity is set to the author's personal address — do not commit
with a different one.
