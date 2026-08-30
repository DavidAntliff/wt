//! wt — git worktree front end: add / rm / list / main / resolve.
//!
//! This library is the implementation behind the `wt` binary, which in turn is
//! driven through the `wt` shell function (see `wt-shell`). The shell function
//! performs the directory changes a child process cannot do to its parent shell.
//!
//! stdout/stderr contract (so the `wt` shell function can capture a cd target):
//!   - stdout = the machine RESULT: a single bare path, for the commands that
//!     have a "resulting location" (add -> new worktree, main -> main clone,
//!     resolve -> matched worktree). `list` is the exception: its stdout is the
//!     table.
//!   - stderr = human narration (progress, confirmations, prompts, errors).
//!
//! Full spec: SPEC.md.

pub mod commands;
pub mod config;
pub mod git;
pub mod theme;
pub mod worktree;

/// Fatal error: `msg` (if any) is printed as "wt: <msg>" to stderr by main,
/// then the process exits with `code`. Ambiguous-match errors (exit 2) narrate
/// their candidates themselves and carry no message.
pub struct Error {
    pub msg: Option<String>,
    pub code: i32,
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error {
            msg: Some(msg.into()),
            code: 1,
        }
    }

    /// Already narrated on stderr; just exit with `code`.
    pub fn silent(code: i32) -> Self {
        Error { msg: None, code }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Whether stderr narration is coloured. Set once in main from the global
/// `--color` option, resolved against STDERR being a terminal (stdout is
/// captured by the `wt` shell function, so it is almost never one).
static NARRATION_COLOUR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_narration_colour(on: bool) {
    NARRATION_COLOUR.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The fixed narration styles — deliberately NOT configurable (unlike the
/// `list` table palette in `theme`/`config`).
pub mod narration {
    use anstyle::{AnsiColor, Style};
    /// Ordinary `wt:` narration.
    pub const INFO: Style = AnsiColor::Cyan.on_default();
    /// The final "worktree ready" line.
    pub const READY: Style = AnsiColor::BrightCyan.on_default();
    /// Warnings worth noticing (e.g. submodules present but skipped).
    pub const NOTICE: Style = AnsiColor::BrightYellow.on_default();
    /// Fatal errors (the `wt: <msg>` line printed just before a non-zero exit).
    pub const ERROR: Style = AnsiColor::BrightRed.on_default();
}

/// Print one narration line to stderr, painted if narration colour is on.
#[doc(hidden)]
pub fn narrate(style: anstyle::Style, text: &str) {
    if NARRATION_COLOUR.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("{style}{text}{}", style.render_reset());
    } else {
        eprintln!("{text}");
    }
}

/// Human narration -> stderr, so stdout stays reserved for the result path.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::narrate($crate::narration::INFO, &format!($($arg)*)) };
}

/// Narration for a warning worth noticing (bright yellow) -> stderr.
#[macro_export]
macro_rules! notice {
    ($($arg:tt)*) => { $crate::narrate($crate::narration::NOTICE, &format!($($arg)*)) };
}

/// Narration for the final "worktree ready" line (bright cyan) -> stderr.
#[macro_export]
macro_rules! ready {
    ($($arg:tt)*) => { $crate::narrate($crate::narration::READY, &format!($($arg)*)) };
}

/// Narration for errors (bright red) -> stderr.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::narrate($crate::narration::ERROR, &format!($($arg)*)) };
}

/// Shorthand for `return Err(Error::new(...))`.
#[macro_export]
macro_rules! fail {
    ($($arg:tt)*) => { return Err($crate::Error::new(format!($($arg)*))) };
}
