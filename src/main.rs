//! Thin clap wrapper; all behaviour lives in the library (see SPEC.md).

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use wt::commands::{self, AddOpts, ListOpts, RmOpts};
use wt::theme::ColorWhen;

#[derive(Parser)]
#[command(
    name = "wt",
    version,
    about = "git worktree front end (implementation behind the `wt` shell function)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Print the built-in colour configuration on stdout and exit. Never
    /// writes a file.
    #[arg(long = "generate-config", exclusive = true)]
    generate_config: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// add a sibling worktree for a branch
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
        /// copy the configured [copy] paths into the new worktree, even if
        /// the config's on-add is false
        #[arg(short = 'c', long)]
        copy: bool,
        /// do not copy the [copy] paths, even if the config's on-add is true
        #[arg(long, conflicts_with = "copy")]
        no_copy: bool,
        /// populate submodules (git submodule update --init --recursive),
        /// even if the config's [submodules] on-add is false
        #[arg(long)]
        submodules: bool,
        /// do not populate submodules, even if the config's on-add is true
        #[arg(long, conflicts_with = "submodules")]
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
        /// when to colour the table
        #[arg(long, value_enum, value_name = "WHEN", default_value = "auto")]
        color: ColorWhen,
    },
    /// copy the configured [copy] paths from the main clone into the current
    /// worktree
    Copy {
        /// overwrite paths that already exist in the worktree
        #[arg(short, long)]
        force: bool,
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
        && !matches!(
            first.as_str(),
            "-h" | "--help" | "-V" | "--version" | "--generate-config"
        )
    {
        argv.insert(1, "list".to_string());
    }

    let cli = Cli::parse_from(argv);
    if cli.generate_config {
        print!("{}", wt::config::DEFAULT_CONFIG);
        return;
    }
    // Bare `wt` (no subcommand) defaults to the worktree list.
    let result = match cli.command {
        None => commands::list(&ListOpts::default()),
        Some(Cmd::Add {
            branch,
            detach,
            base,
            path,
            copy,
            no_copy,
            submodules,
            no_submodules,
            no_cd,
        }) => commands::add(&AddOpts {
            branch,
            detach,
            base,
            path,
            // -c => Some(true), --no-copy => Some(false), neither => follow config.
            copy: match (copy, no_copy) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            },
            // --submodules => Some(true), --no-submodules => Some(false),
            // neither => follow config.
            submodules: match (submodules, no_submodules) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            },
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
            color,
        }) => commands::list(&ListOpts {
            porcelain,
            absolute,
            size,
            git,
            color,
        }),
        Some(Cmd::Copy { force }) => commands::copy(force),
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
