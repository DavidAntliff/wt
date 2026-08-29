//! Subprocess helpers around the `git` binary, honouring the stdout/stderr
//! contract: nothing a child prints may ever land on OUR stdout.

use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Error, Result, fail};

fn command(args: &[&str], cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd
}

/// Run a command, return stripped stdout; error on failure.
pub fn capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let out = command(args, cwd)
        .output()
        .map_err(|e| Error::new(format!("failed to run {}: {e}", args[0])))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::new(if stderr.is_empty() {
            format!("command failed: {}", args.join(" "))
        } else {
            stderr
        }));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run a command, error on failure.
///
/// The child's stdout is redirected to OUR stderr so git's chatter (e.g.
/// "HEAD is now at …", "Deleted branch …") never pollutes our stdout, which is
/// reserved for the single result path the `wt` shell function captures.
pub fn run(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let stdout_as_stderr = std::io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::inherit());
    let status = command(args, cwd)
        .stdout(stdout_as_stderr)
        .status()
        .map_err(|e| Error::new(format!("failed to run {}: {e}", args[0])))?;
    if !status.success() {
        return Err(Error::new(format!("command failed: {}", args.join(" "))));
    }
    Ok(())
}

/// Tolerant query: stripped stdout on success, `None` on any failure.
pub fn query(args: &[&str], cwd: Option<&Path>) -> Option<String> {
    let out = command(args, cwd).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn require_worktree(cwd: &Path) -> Result<()> {
    if capture(&["git", "rev-parse", "--is-inside-work-tree"], Some(cwd))? != "true" {
        fail!("not inside a git work tree");
    }
    Ok(())
}

/// The main clone = parent of the shared .git dir. Stable from anywhere.
pub fn main_clone_of(cwd: &Path) -> Result<PathBuf> {
    let common = PathBuf::from(capture(
        &["git", "rev-parse", "--git-common-dir"],
        Some(cwd),
    )?);
    let common = if common.is_absolute() {
        common
    } else {
        cwd.join(common)
    };
    let common = common.canonicalize().unwrap_or(common);
    Ok(common.parent().unwrap_or(&common).to_path_buf())
}

/// Canonicalize where the path exists; otherwise just absolutize (like
/// Python's `Path.resolve(strict=False)`), so comparisons behave under
/// symlinked temp dirs but paths-to-be-created still work.
pub fn resolve(path: &Path) -> PathBuf {
    path.canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}
