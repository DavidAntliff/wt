# wt specification

A git-worktree front end in two parts:

- **the `wt` binary** (this crate) — implements `add` / `rm` / `list` / `main` /
  `resolve`;
- **the `wt` shell function** (`wt-shell`, sourced into bash) — the user-facing
  command. It forwards plain actions to the binary and, for the directory-changing
  commands (`wt main` / `cd` / `add` / `rm`), captures the path the binary prints on
  stdout and performs the `cd` itself — the one thing a child process cannot do to
  its parent shell.

The sourced function shadows the binary on `PATH` and reaches it with `command wt`.
The binary also works standalone (unsourced): the path-emitting commands then just
PRINT the resulting path instead of cd-ing to it.

**Status:** ported from the original Python + bash implementation.
**Shells:** bash only for now; zsh later.

## The model

One worktree == one branch == one IDE window == one coding agent. Worktrees are
created as SIBLINGS of the main clone:

```
~/work/
  repo/                     # the main clone
  wt-repo-dev-fix-foo.git/  # worktree for branch dev/fix-foo
  wt-repo-v1.2.3.git/       # detached worktree at tag v1.2.3
```

The main clone is always found as the parent of the shared `.git` directory
(`git rev-parse --git-common-dir`), so every command works from any worktree and
any subdirectory.

## stdout / stderr contract

This is the interface between the binary and the shell function; violating it
breaks `wt` (the function would `cd` into whatever lands on stdout).

- **stdout = the machine RESULT** — a single bare path — for the commands with a
  "resulting location": `add` (new worktree), `main` (main clone), `resolve`
  (matched worktree), and `rm` (the safe dir, only when it removed the shell's
  cwd). `list` is the deliberate exception: its stdout is the human table (or
  porcelain with `-p`), because nothing cds to it.
- **stderr = all human narration**: progress, confirmations, prompts, errors.
- A child git command's stdout is redirected to OUR stderr, so git's chatter
  ("HEAD is now at …", "Deleted branch …") can never pollute the result.
- Errors print `wt: <msg>` to stderr and exit 1 (an ambiguous `resolve`/`rm`
  query exits 2 — see `resolve`).

## Binary commands

```
wt add  [-d] [-b REF] [-p DIR] [-c|--no-copy] [-s|--no-submodules] [--no-cd] [BRANCH]
wt rm   [-f] [-d] [PATH|QUERY]
wt list [-p|--porcelain] [-a|--absolute] [-s|--size] [-g|--git]
wt <any> [--color WHEN]       # global: list's table and stderr narration
wt main
wt resolve QUERY
wt copy [-f|--force]
wt            # no command -> list (exit 0)
wt -s|-g|...  # leading flag -> list with those flags (wt -sg works)
```

### add [BRANCH]

Add a git worktree as a SIBLING of the main clone, ready to open in an IDE + a
fresh coding agent. BRANCH is required unless `-d` is given.

Which commit the worktree starts at (first matching case):

1. BRANCH exists locally → check it out (`--base` ignored, with a notice)
2. BRANCH exists on exactly ONE remote → check it out tracking that remote (the
   git-worktree DWIM), UNLESS `--base` was given (which forces case 3). Reflects
   the last fetch; fetch first if the remote branch is newer.
3. otherwise → create a new branch off the base ref

Options:

- `-d, --detach` — create a DETACHED worktree: no branch is created or checked
  out, so none of the three cases above apply and `wt rm` later has no branch to
  offer to delete. BRANCH is then a COMMIT-ISH to detach at (tag, sha, branch
  name…), defaulting to HEAD. That HEAD is resolved in the CURRENT worktree, not
  the main clone — matching what plain `git worktree add -d <path>` would check
  out if run from here — and the resolved sha is what gets passed to git.
  Rejects `--base` (the argument already names the commit).
- `-b, --base REF` — base a NEW branch on REF (forces case 3). Default: the
  branch (or commit, if detached) currently checked out where `wt` is run.
  Ignored, with a notice, if BRANCH exists.
- `-p, --path DIR` — create the worktree at DIR instead of the default sibling
  path. Relative paths resolve against the current dir.
- `-c, --copy` — copy the configured `[copy]` paths into the new worktree even
  if the config's `on-add` is false; `--no-copy` suppresses the copy even if it
  is true (the two conflict). With neither flag, the config's `on-add` decides.
  Copying with an empty `paths` list is a notice, not an error.
- `-s, --submodules` — populate submodules (`git submodule update --init
  --recursive`) in the new worktree even if the config's `[submodules] on-add`
  is false; `--no-submodules` skips it even if it is true (the two conflict).
  With neither flag, the config's `on-add` (default false) decides. When
  submodules are skipped but the new worktree has a `.gitmodules` at its root,
  a stderr notice says so and how to populate them.
- `--no-cd` — create the worktree but do NOT cd into it: suppress the stdout
  path so the `wt` shell function stays put, and print
  "...but shell remains in current worktree!".

Default worktree path (no `-p`): `<container>/wt-<base>-<normalised-name>.git`,
where the main clone is found via `git rev-parse --git-common-dir` (its parent),
`<container>` is the main clone's parent dir, and `<base>` is the clone's dir
name minus a trailing `.git`. `<name>` is BRANCH, or — for a bare `add -d` with
no argument to name it after — the abbreviated commit. Name normalisation (dir
name only; the real branch is untouched): any run of chars outside
`[A-Za-z0-9._-]` becomes a single `-`, leading/trailing `-` stripped.

Narration → stderr; the new worktree path → stdout (so `wt add` cds in), except
with `--no-cd` (no stdout path).

### rm [PATH|QUERY]

Remove a worktree (cleaning up git's bookkeeping, unlike plain `rm`). The
argument defaults to `.` (the current worktree) and may be any path inside the
target. If it is NOT an existing directory, it is treated as a branch/dir QUERY
and resolved to a worktree exactly like `wt cd` (same exact/substring rules,
same ambiguous=exit-2 and no-match=exit-1 behaviour); this lookup lists the
repo's worktrees, so it must be run from inside a worktree of the repo.

- `-f, --force` — discard uncommitted/untracked changes AND skip ALL prompts.
  Keeps the branch unless `-d` is also given.
- `-d, --delete` — delete the branch with `git branch -d` (not forced) without
  prompting; unmerged → notice, worktree still removed; no-op for a detached
  worktree.

Refuses to remove the main clone. Prints the worktree/branch/main clone and any
dirty files (a detached worktree's "branch" shows as `(detached at <short-sha>)`);
aborts if dirty without `-f`; otherwise prompts (y/yes) to remove the worktree
unless `-f`. `worktree remove --force` is always used (git refuses with
submodules present otherwise); dirtiness is gated separately.

Branch deletion (from the main clone), after the worktree is removed:

| state          | branch                                             |
|----------------|----------------------------------------------------|
| detached       | nothing to delete                                  |
| `-d`           | delete, no prompt ("do it anyway", `-d` beats `-f`)|
| `-f` (no `-d`) | keep, no prompt (force defaults to keeping it)     |
| neither        | interactively offer "Also delete branch 'X'? [y/N]"|

A declined/kept branch prints how to delete it later.

stdout: the main-clone path IFF the removed worktree contained the current
directory (so the `wt` shell function can cd out of the now-deleted dir,
avoiding a stranded shell where `getwd`/git — and thus `wt main` — fail);
otherwise nothing.

### list

Table of every worktree of the repo (main clone first, tagged `[main]`; the
current worktree tagged `[cwd]`). Branch column shows the branch, or
`(detached at <short-sha>)` / `(bare)` / `(unknown)`, where `<short-sha>` comes
from `git rev-parse --short` (so it is as wide as this repo needs, not a fixed
slice); if that abbreviation query fails (e.g. a prunable worktree whose commit
is gone), the label falls back to plain `(detached)` rather than aborting the
listing. This is also what bare `wt` runs — and a leading FLAG with no subcommand
(`wt -s`, `wt -g`, `wt -sg`) is rewritten to `list` with those flags, so the
list options work without typing `list`.

- `-p, --porcelain` — `<path>\t<branch>` per line, no header/markers, git order
  (always absolute, for machine consumers). `-s` appends a `<size>` field; `-g`
  appends `<status>\t<merged>\t<upstream>\t<last>` — after `<size>` if both are
  given.
- `-a, --absolute` — show absolute paths in the table (default: relative to cwd).
- `-s, --size` — add a right-aligned SIZE column (between BRANCH and the
  markers) with each worktree's on-disk usage, as reported by `du -sh
  <worktree>` — i.e. the whole directory: working files, submodule checkouts,
  build output, and (for the main clone) the shared `.git` object store. Off by
  default because du walks every file, which is slow on big trees. A worktree du
  cannot fully read still reports du's partial total, or `?` if du printed
  nothing. The size-band colouring parses du's C-locale figures (`1.2G`); in a
  locale with a decimal comma (`1,2G`) the size still displays but is not
  colour-classified (deliberate: du is not forced to `LC_ALL=C` for now).
- `-g, --git` — add four git-state columns (between SIZE and the markers),
  answering "is it OK to remove this worktree?":
  - `STATUS` — working-tree cleanliness from `git status --porcelain`: `clean`,
    or counts split into tracked changes vs untracked files (`3 mod`, `2 untr`,
    `3 mod, 2 untr`). `-` for bare.
  - `MERGED` — `merged` if the worktree's HEAD is reachable from the MAIN
    worktree's HEAD, else `+N` (its unmerged commit count, via `rev-list --count
    main..HEAD`). `-` on the main/bare row; `?` if a HEAD is unknown.
  - `UPSTREAM` — ahead/behind the branch's tracking ref (`rev-list --left-right
    --count @{u}...HEAD`): `ok` (in sync), `ahead N`, `behind N`,
    `ahead N, behind M`, `none` (no upstream), `-` (detached/bare — no branch).
    ANY failed `@{u}` query renders `none` — a missing upstream is by far the
    usual cause and is not distinguished from other rev-list failures, so this
    column never shows `?`.
  - `LAST` — the HEAD commit's relative committer date (`log -1 --format=%cr`,
    e.g. `3 days ago`).

  Safe to remove = STATUS clean AND (MERGED merged, or UPSTREAM ok/behind N —
  i.e. no local-only commits). Dirty, or `+N` with UPSTREAM none/ahead, means
  work would be lost. Cheap (refs/index only), unlike `-s`: no tree walk. Failed
  queries render as values (`?`), never abort the listing.

- `--color WHEN` — a GLOBAL option (accepted by every command; documented here
  because the table is its main subject): `auto` (default: only when the
  stream is a terminal, `NO_COLOR` is unset, and `TERM` is not `dumb`),
  `always`, `never`. For the table `auto` looks at stdout; for stderr
  narration (see **Narration colour** below) it looks at stderr.
  Porcelain output is never coloured. All colours and thresholds live in one
  `Theme` struct (`src/theme.rs`), nothing is hard-coded at the paint site;
  the defaults:

  | element | colour |
  |---------|--------|
  | column headers | cyan bold |
  | PATH | bright-white |
  | BRANCH | white |
  | marker brackets `[]` | bright-white; `main` bright-yellow, `cwd` bright-green |
  | SIZE | bright-white; > 1 GiB bright-yellow; > 10 GiB bright-red |
  | STATUS | `clean` green; `N mod` red; `N untr` yellow (parts painted separately) |
  | MERGED | `merged` green; `+N` yellow |
  | UPSTREAM | `ok` green; `none` white; ahead/behind unpainted |
  | LAST | ≤ 3 days green; < 1 week yellow; ≥ 1 week red |

  Unknown cell values (`-`, `?`) are never painted. Column widths are computed
  from plain text before painting, so escapes cannot affect alignment.

  The palette is configurable — see **Configuration** below.

This command's stdout IS its output (table/porcelain).

## Narration colour

Every command's stderr narration is coloured too, honouring the global
`--color` option resolved against STDERR (stdout is captured by the shell
function, so it is almost never a terminal). The styles are fixed —
deliberately NOT configurable, unlike the table palette:

| narration | colour |
|-----------|--------|
| ordinary `wt:` lines | cyan |
| warnings (submodules skipped, config problems, branch NOT deleted, …) | bright-yellow |
| the final `wt: worktree ready` line | bright-cyan |
| fatal errors (the `wt: <msg>` line before a non-zero exit, ambiguous-match headers) | bright-red |
| interactive y/N prompts | bright-magenta |

The narration wt-shell prints itself follows the same rules — `wt: cd <dir>`
after `rm` moves the shell to safety is cyan; the dead-cwd recovery lines
("… is gone — moved to …") are warnings, bright-yellow — honouring a
`--color` among the args and the same stderr-auto resolution.

### main

Print the main-clone root (parent of the shared `.git`) to stdout. `wt main` cds
to it. `parent`, the command's former name, is still accepted as an undocumented
alias (hidden from `--help`).

### copy [-f]

Copy the paths listed under `[copy]` in the config file (untracked per-worktree
material like IDE config) from the MAIN CLONE into the CURRENT worktree's root
(resolved via `git rev-parse --show-toplevel`, so it works from any subdir).
The same set is what `add` copies when copy-on-add is enabled. Directories are
copied recursively; a plain file's parent directories are created as needed.

- `-f, --force` — overwrite paths that already exist in the worktree (each is
  removed first, so the result is a clean copy of the main clone's — a true
  overwrite, not a merge).

Refuses to run from the main clone itself (it is the source, not a target).
Errors if `paths` is empty (with a pointer at the config), or — before touching
anything, so the run is all-or-nothing — if any destination already exists and
`-f` was not given. A configured path absent from the main clone is a stderr
notice, not an error. No stdout path.

(This command replaced the earlier `wt idea`, which hardcoded `.idea/`; there
is no alias.)

### resolve QUERY

Map QUERY to a single worktree by its branch name OR its worktree dir basename
(so detached worktrees, which have no branch, are still reachable by path). A
reusable primitive: the `wt cd QUERY` shell command calls it and cds to the
result (this command only PRINTS the path — it never chdirs — so other tooling
can use it too). Prefer an EXACT match (branch or dir); else case-insensitive
SUBSTRING match on either.

- unique → print the worktree path to stdout, exit 0
- ambiguous → list "branch → path" candidates to stderr, exit 2
- none → error to stderr (with a `wt add QUERY` hint), exit 1

## Configuration

Colours, the SIZE/LAST thresholds, the `[copy]` path set, and the
`[submodules]` switch. The model is
slogs': they come from a config file and nowhere else; a missing file is
normal and means the built-in defaults.

- **Location**: `$WT_CONFIG`, else `$XDG_CONFIG_HOME/wt/config.toml`, else
  `~/.config/wt/config.toml`.
- **`wt --generate-config`** prints the built-in configuration on stdout (never
  writes a file). It is the single source of truth: the built-in defaults are
  produced by parsing that same template, so the two cannot drift.
- **Format**: TOML, one `[colour]` table (`[color]` is accepted too). Keys:
  `header`, `path`, `branch`, `marker-brackets`, `marker-main`, `marker-cwd`,
  `size`, `size-warn`, `size-alert`, `status-clean`, `status-modified`,
  `status-untracked`, `merged`, `unmerged`, `upstream-ok`, `upstream-none`,
  `last-fresh`, `last-aging`, `last-old`. There is deliberately no
  `[colour.values]`-style section.
- **`[copy]`** configures `wt copy` and copy-on-add: `on-add` (boolean,
  default false) makes `wt add` copy automatically; `paths` (array of strings,
  default empty) lists what to copy, relative to the repository root. Entries
  that are absolute or leave the tree (`..`) are ignored with a warning.
- **`[submodules]`** configures `wt add`'s submodule handling: `on-add`
  (boolean, default false) makes `wt add` run `git submodule update --init
  --recursive` in the new worktree.
- **`[thresholds]`** holds where the SIZE and LAST colours switch over, as
  whole non-negative numbers: `size-warn` (MiB, default 1024) and `size-alert`
  (MiB, default 10240) — SIZE bigger than these uses the warn/alert colour —
  and `last-fresh` (days, default 3: this old or newer is fresh) and
  `last-aging` (days, default 7: this old or older is old; between the two is
  aging).
- **Style specs** use the slogs grammar: one string per key, space-separated
  attributes (`bold dim italic underline reverse`) plus at most one colour —
  an ANSI name (`red`, `bright-white`, …), a 0-255 palette index, or
  `#rrggbb`/`#rgb` hex — in any order; `default` means the terminal's own
  foreground. Hex is approximated to the nearest 256-palette entry on
  terminals without truecolor. xterm palette names (`MistyRose1`, …) are not
  accepted yet.
- **A user config overlays the defaults key by key**: setting one colour
  leaves every other default in place.
- **Problems are warnings, never failures**: unknown sections/keys, bad
  specs, unreadable or invalid TOML each print a `wt:` warning on stderr (even
  when colour is off) and the listing still runs. Exception: `list -p` never
  loads the config at all (porcelain is never coloured), so config warnings
  appear only in table mode and in the other config-reading commands.
- `--color auto` resolution follows cargo: `CLICOLOR_FORCE` beats everything,
  then `NO_COLOR`, then whether stdout is a terminal that supports colour.

## The shell function (wt-shell)

Must be SOURCED, not executed. Add to `~/.bashrc` (or equivalent):

```sh
source ~/.local/bin/wt-shell
```

The function reaches the shadowed binary with `command wt`, so the pair need no
configured paths — they only both have to be on `PATH`.

Dispatch table (on the first word). The four directory-changing commands and the
top-level help are intercepted; everything else (including unknown input) is
forwarded to the binary.

| word | behaviour |
|------|-----------|
| `-h` \| `--help` \| `help` | Print `wt`'s OWN curated overview — the commands as invoked through `wt`, including the shell-only `cd`. It does NOT defer to `wt --help` (which lists `resolve`, not `cd`). |
| `main` \| `parent` | Shell-intercepted. Run `wt main`, capture the main-clone root from stdout, and cd to it. `parent` is the command's former name: still accepted, no longer documented anywhere. |
| `cd [QUERY]` | Shell-intercepted. With QUERY: run `wt resolve QUERY` and cd to the printed path. With NO query, or with the literal query `main`, behave like `wt main`. (`main` is special-cased rather than left to `resolve`, so it lands on the main clone deterministically instead of depending on that clone happening to be on a branch named `main` — a fuzzy match could otherwise pick up e.g. `wt-…-maintenance.git`. Nothing legitimate is shadowed: git will not let a linked worktree hold the main clone's branch anyway.) The `cd` verb lives only in this wrapper; `resolve` is the binary's reusable primitive. |
| `add` | Shell-intercepted. Run `wt add …` (narration streams via stderr), capture the new worktree path from stdout, and cd into it. All add flags pass through; with `--no-cd` the binary prints no path, so the shell stays put. |
| `rm` | Run `wt rm …`. rm can delete the directory the shell is in; in that case the binary prints a safe dir (the main clone) on stdout and `wt` cds there, so the shell is never stranded in a deleted directory. rm's prompts still work: they are on stderr and stdin stays attached under the capture. |
| anything else (`list`, `copy`, empty, unknown) | Forwarded verbatim with stdin/stdout/stderr attached. `copy` syncs the configured `[copy]` paths into the current worktree; empty → the binary defaults to `list`; a leading list flag (`wt -s`, `wt -g`, `wt -sg`) → the binary rewrites it to `list`; an unknown command → clap's "unrecognized subcommand" error. Nothing here changes cwd. |

**Dead-cwd recovery.** Before dispatching, every `wt` invocation checks that
the current directory still exists; a worktree can be removed out from under
a shell by ANOTHER shell's `wt rm` (or a plain `rm -rf`), after which
`getcwd()` fails and everything — git, `wt cd`, the escape hatches — breaks.
When cwd is dead, `_wt_recover` cds to the repo's main clone if known, else
to the nearest existing ancestor of the dead path, narrates where it went
(a bright-yellow warning, honouring `--color` among the args), and the
requested command then
runs normally from there. The main clone is remembered in the shell variable
`_wt_main_clone`, refreshed at the top of every `wt` invocation while cwd is
alive (`_wt_remember_main`, one extra `wt main` subprocess; outside a repo
the previous value is kept). The variable is per-shell and can be stale — if
the user manually cd'd to another repo and never ran `wt` there, recovery
moves to the last repo `wt` saw; the narration says exactly where the shell
went. `_wt_recover` is a silent no-op with exit 0 when cwd is alive, so it
can also be run before every prompt for self-healing without typing `wt`:

```sh
PROMPT_COMMAND="_wt_recover${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
```

Per-command help (`wt <cmd> -h` / `--help`): for the intercepting branches
(main/cd/add/rm), a help flag in the args is detected first and the call is
FORWARDED (`wt cd -h` → `wt resolve --help`) rather than captured — otherwise
the help text, printed to stdout, would be mistaken for a path and cd'd into.

The cd-style branches (main / cd / add) capture stdout into `target` and the
exit status into `st`; they cd only when `st == 0` and `target` is non-empty,
otherwise return `st`. (`rm` differs: it cds whenever a target was printed, and
always returns the binary's status.)

Worktree switching is the explicit `wt cd QUERY`; there is deliberately no bare
`wt <branch>` shorthand (it caused unknown words to be mis-handled as branch
queries). A branch named after a subcommand is still reachable: `wt cd <name>`
matches on the branch, and `wt resolve <name>` prints its path.

Executing `wt-shell` directly (not sourcing it) prints usage and exits 0.

## Errors (exit 1, unless noted)

- Not inside a git work tree
- `add`: no BRANCH and no `-d` / `-d` with `--base` / name normalises to empty /
  target dir exists / commit-ish does not resolve / git command fails
- `rm`: arg is neither a dir nor a matching branch/dir query (no-match exit 1,
  ambiguous exit 2) / not inside a worktree / target is the main clone / dirty
  without `-f` / confirmation declined / `worktree remove` fails (a failed
  `branch -d` is a non-fatal stderr notice)
- `resolve`: no match (exit 1); ambiguous match (exit 2) — used by `wt cd`
- `copy`: run from the main clone / no paths configured / a destination
  already exists and no `-f`
- clap usage errors exit 2 (as argparse did)

## Notes

- Lists/resolves worktrees of ONE repo (the one you're in); does not recurse
  into submodules' own worktrees.
- New branches start at the base ref's tip; fetch/pull first (or pass
  `-b origin/<branch>`) for the latest upstream.
- History: this replaced earlier standalone `wt-add`/`wt-rm`/`wt-list`
  executables and a `wt-parent` function; `main` was called `parent` until the
  rename, and `wt parent` still works but is not documented in any help output.
