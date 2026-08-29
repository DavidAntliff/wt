//! Thin clap wrapper; all behaviour lives in the library (see SPEC.md).

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use wt::commands::{self, AddOpts, ListOpts, RmOpts};

#[derive(Parser)]
#[command(
    name = "wt",
    version,
    about = "git worktree front end (implementation behind the `wt` shell function)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// add a sibling worktree for a branch, with submodules
    Add {
        /// branch name, e.g. dev/fix-foo; with -d, a commit-ish to detach at
        /// (default: HEAD)
        branch: Option<String>,
        /// create a detached worktree (no branch) at the commit-ish
        #[arg(short, long)]
        detach: bool,
        /// base a new branch on REF (default: the current branch)
        #[arg(short, long, value_name = "REF")]
        base: Option<String>,
        /// worktree directory (overrides the default sibling path)
        #[arg(short, long, value_name = "DIR")]
        path: Option<PathBuf>,
        /// skip submodule checkout (default: check them out)
        #[arg(long)]
        no_submodules: bool,
        /// create the worktree but do not cd into it (shell stays put)
        #[arg(long)]
        no_cd: bool,
    },
    /// remove a worktree
    Rm {
        /// worktree path, or a branch/dir query (matched like `wt cd`) if it is
        /// not an existing directory (default: the current worktree)
        #[arg(default_value = ".")]
        path: String,
        /// discard uncommitted/untracked changes and skip all prompts (keeps
        /// the branch unless -d is also given)
        #[arg(short, long)]
        force: bool,
        /// delete the branch too (git branch -d), without prompting; otherwise
        /// `wt rm` interactively offers to delete it
        #[arg(short, long)]
        delete: bool,
    },
    /// list the worktrees of the current repo
    List {
        /// machine-readable output: '<path>\t<branch>' per line, no markers
        #[arg(short, long)]
        porcelain: bool,
        /// show absolute paths (default: relative to cwd)
        #[arg(short, long)]
        absolute: bool,
        /// add a SIZE column with each worktree's disk usage (du -sh)
        #[arg(short, long)]
        size: bool,
        /// add git-state columns: STATUS (clean / N mod, N untr), MERGED
        /// (merged into the main worktree's branch, or +N unmerged commits),
        /// UPSTREAM (ok / ahead N / behind N / none), LAST (last commit's
        /// relative date). Safe to remove = clean AND (merged, or nothing
        /// local-only upstream: ok / behind N)
        #[arg(short, long)]
        git: bool,
    },
    /// print the main-clone root (for `wt main`)
    Main,
    /// back-compat alias for `main`, hidden from --help
    #[command(hide = true)]
    Parent,
    /// print the worktree matching QUERY by branch or path (primitive for `wt cd`)
    Resolve {
        /// exact or substring match against branch name or worktree dir
        query: String,
    },
}

fn main() {
    // Flags with no subcommand (`wt -s`, `wt -g`, `wt -sg`, ...) mean `list`
    // with those flags. -h/--help/-V/--version stay top-level.
    let mut argv: Vec<String> = std::env::args().collect();
    if let Some(first) = argv.get(1)
        && first.starts_with('-')
        && !matches!(first.as_str(), "-h" | "--help" | "-V" | "--version")
    {
        argv.insert(1, "list".to_string());
    }

    let cli = Cli::parse_from(argv);
    // Bare `wt` (no subcommand) defaults to the worktree list.
    let result = match cli.command {
        None => commands::list(&ListOpts::default()),
        Some(Cmd::Add {
            branch,
            detach,
            base,
            path,
            no_submodules,
            no_cd,
        }) => commands::add(&AddOpts {
            branch,
            detach,
            base,
            path,
            submodules: !no_submodules,
            no_cd,
        }),
        Some(Cmd::Rm {
            path,
            force,
            delete,
        }) => commands::rm(&RmOpts {
            path,
            force,
            delete,
        }),
        Some(Cmd::List {
            porcelain,
            absolute,
            size,
            git,
        }) => commands::list(&ListOpts {
            porcelain,
            absolute,
            size,
            git,
        }),
        Some(Cmd::Main | Cmd::Parent) => commands::main_cmd(),
        Some(Cmd::Resolve { query }) => commands::resolve(&query),
    };

    if let Err(e) = result {
        if let Some(msg) = e.msg {
            eprintln!("wt: {msg}");
        }
        std::process::exit(e.code);
    }
}
