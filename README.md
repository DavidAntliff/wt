# wt

A git worktree front end: create, switch between, list, and clean up sibling
worktrees with one short command — including the `cd`, which plain git tooling
cannot do for you.

The working model is one worktree per branch, kept as siblings of the main
clone:

```
~/work/
  repo/                       # the main clone
  wt-repo-dev-fix-foo.git/  # worktree for branch dev/fix-foo
  wt-repo-v1.2.3.git/         # detached worktree at tag v1.2.3
```

One worktree == one branch == one IDE window == one coding agent. `wt` handles
the naming, the submodules, the branch bookkeeping, and — because it is a
sourced shell function — your shell ends up *in* the worktree you just made or
asked for:

```
$ wt add dev/fix-foo
wt: created branch dev/fix-foo based on main
wt: worktree ready at /home/you/work/wt-repo-dev-fix-foo.git
$ pwd
/home/you/work/wt-repo-dev-fix-foo.git

$ wt cd main            # back to the main clone
$ wt cd fix             # fuzzy-match your way to any worktree
$ wt rm fix-foo         # remove it (and optionally its branch) when done
```

`wt list -g` answers the question a pile of worktrees always raises — *which of
these can I delete?*

```
$ wt -g
PATH                          BRANCH         STATUS   MERGED  UPSTREAM  LAST
../repo                       main           clean    -       ok        2 hours ago   [main] [cwd]
../wt-repo-dev-fix-foo.git  dev/fix-foo  2 mod    +3      ahead 3   10 minutes ago
../wt-repo-old-spike.git      old-spike      clean    merged  none      3 weeks ago
```

`old-spike` is clean and merged: safe to `wt rm`. `fix-foo` has local work.

The table is coloured when stdout is a terminal (green = safe / fresh, yellow =
attention, red = work you'd lose); `--color always|never|auto` overrides, and
`NO_COLOR` is respected.

## Install

Two pieces: the `wt` binary (Rust) and the `wt` shell function that wraps it
(`wt-shell`, bash). The function is what lets `wt` change your shell's
directory; it shadows the binary and calls it with `command wt`.

```sh
cargo build --release
install -m 755 target/release/wt ~/.local/bin/
install -m 644 wt-shell ~/.local/bin/
```

or, with [just](https://github.com/casey/just): `just install`.

Then add to your `~/.bashrc`:

```sh
source ~/.local/bin/wt-shell
```

Bash only for now. Without the sourced function the binary still works — the
directory-changing commands just print the resulting path instead of cd-ing to
it, which also makes it usable from scripts and other tooling.

## Commands

```
wt                       list all worktrees (same as `wt list`)
wt list [-p] [-a] [-s] [-g] [--color WHEN]
wt -s | -g | -sg         shorthand: list flags work without the `list` word
wt cd <query>            cd to the worktree whose branch/path fuzzy-matches <query>
wt main                  cd to the main clone
wt add [opts] <branch>   create a sibling worktree for <branch> and cd into it
wt add -d [commit-ish]   ...or a detached worktree (no branch), default HEAD
wt rm [opts] [path]      remove a worktree (offers to delete its branch too)
wt idea [-f]             copy the main clone's .idea/ into the current worktree
```

Run `wt <command> -h` for a command's own options.

### add

`wt add <branch>` does the right thing for each kind of branch:

- exists locally → checked out into the new worktree
- exists on exactly one remote → checked out with tracking (so a teammate's
  branch, or one you pushed earlier, is not silently forked)
- otherwise → a new branch, based on whatever you had checked out (`-b REF` to
  base it elsewhere)

Submodules are populated by default (`--no-submodules` to skip). `-p DIR`
overrides the default sibling path; `-i` seeds the new worktree with the main
clone's `.idea/` (IntelliJ config); `--no-cd` creates the worktree but leaves
your shell where it is. `wt add -d v1.2.3` gives a branchless worktree detached
at a tag/commit — handy for builds and bisects.

### rm

`wt rm` (from inside a worktree) or `wt rm <query>` removes a worktree with
git's bookkeeping cleaned up, refuses to touch the main clone, never discards
uncommitted work without `-f`, and offers to delete the now-unused branch
(`-d` to delete it without asking). If it removed the directory your shell was
standing in, your shell is moved to the main clone instead of being stranded.

### idea

`wt idea` copies the main clone's `.idea/` (IntelliJ per-project config, which
is per-directory and so doesn't follow you into a new worktree) into the
current worktree, for worktrees created without `add -i`. It refuses to
overwrite an existing `.idea/` unless you pass `-f`.

### cd / resolve

`wt cd <query>` matches `<query>` against branch names *and* worktree directory
names — exact match first, then case-insensitive substring. One match: you're
there. Several: they're listed and nothing happens. The underlying primitive,
`wt resolve <query>`, just prints the path, for use in scripts.

## Design notes

The full behaviour — including the stdout/stderr contract that makes the shell
function work — is specified in [SPEC.md](SPEC.md). The short version: the
binary's stdout carries only a machine result (a single path) for the commands
the shell function cds after; every human-facing message goes to stderr.

This is a Rust port of an earlier personal Python + bash implementation.

## License

MIT — see [LICENSE](LICENSE).
