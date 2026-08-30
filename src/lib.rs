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

/// Human narration -> stderr, so stdout stays reserved for the result path.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { eprintln!($($arg)*) };
}

/// Shorthand for `return Err(Error::new(...))`.
#[macro_export]
macro_rules! fail {
    ($($arg:tt)*) => { return Err($crate::Error::new(format!($($arg)*))) };
}
