//! Colour for `list`'s table. Every colour and threshold is a field of
//! [`Theme`], never hard-coded at the paint site — the palette is meant to be
//! tweaked (and later loaded from configuration).
//!
//! Widths are always computed from PLAIN cell text; painting wraps the text
//! after padding is decided, so escapes can never enter a width calculation.

use anstyle::{AnsiColor, Style};

/// When to colour the table (mirrors slogs / grep / ls).
#[derive(Clone, Copy, Default, PartialEq, clap::ValueEnum)]
pub enum ColorWhen {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorWhen {
    /// Auto = stdout is a terminal, NO_COLOR is unset, TERM is not "dumb".
    pub fn enabled(self) -> bool {
        use std::io::IsTerminal;
        match self {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => {
                std::io::stdout().is_terminal()
                    && std::env::var_os("NO_COLOR").is_none()
                    && std::env::var_os("TERM").is_none_or(|t| t != "dumb")
            }
        }
    }
}

/// The palette and the thresholds that pick between its entries.
pub struct Theme {
    pub header: Style,

    // PATH / BRANCH columns.
    pub path: Style,
    pub branch: Style,

    // [main] / [cwd] markers: the brackets, and each word.
    pub marker_brackets: Style,
    pub marker_main: Style,
    pub marker_cwd: Style,

    // SIZE: normal, then escalating by on-disk size.
    pub size: Style,
    pub size_warn: Style,
    pub size_alert: Style,
    pub size_warn_bytes: u64,
    pub size_alert_bytes: u64,

    // STATUS: 'clean' / 'N mod' / 'N untr' (painted per part).
    pub status_clean: Style,
    pub status_mod: Style,
    pub status_untr: Style,

    // MERGED: 'merged' / '+N'.
    pub merged_ok: Style,
    pub merged_unmerged: Style,

    // UPSTREAM: 'ok' / 'none' (ahead/behind stay unpainted).
    pub upstream_ok: Style,
    pub upstream_none: Style,

    // LAST: by age of the HEAD commit.
    pub last_fresh: Style,
    pub last_aging: Style,
    pub last_old: Style,
    pub last_fresh_days: u64,
    pub last_aging_days: u64,
}

const CYAN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));
const WHITE: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::White)));
const RED: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
const GREEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
const YELLOW: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
const GIB: u64 = 1 << 30;

impl Default for Theme {
    fn default() -> Self {
        Theme {
            header: CYAN,
            path: WHITE,
            branch: RED.italic(),
            marker_brackets: WHITE,
            marker_main: YELLOW,
            marker_cwd: GREEN,
            size: WHITE,
            size_warn: YELLOW,
            size_alert: RED,
            size_warn_bytes: GIB,
            size_alert_bytes: 10 * GIB,
            status_clean: GREEN,
            status_mod: RED,
            status_untr: YELLOW,
            merged_ok: GREEN,
            merged_unmerged: RED,
            upstream_ok: GREEN,
            upstream_none: WHITE,
            last_fresh: GREEN,
            last_aging: YELLOW,
            last_old: RED,
            last_fresh_days: 3,
            last_aging_days: 7,
        }
    }
}

/// Wrap `text` in `style` (a no-op for the empty style).
fn painted(style: Style, text: &str) -> String {
    if style == Style::new() {
        text.to_string()
    } else {
        format!("{style}{text}{reset}", reset = style.render_reset())
    }
}

/// Approximate bytes from a `du -h` figure like "80M" / "1.2G" / "512".
fn du_bytes(s: &str) -> Option<u64> {
    let (num, unit) = match s.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&s[..i], c.to_ascii_uppercase()),
        _ => (s, 'B'),
    };
    let n: f64 = num.parse().ok()?;
    let mult: u64 = match unit {
        'B' => 1,
        'K' => 1 << 10,
        'M' => 1 << 20,
        'G' => 1 << 30,
        'T' => 1u64 << 40,
        _ => return None,
    };
    Some((n * mult as f64) as u64)
}

/// Approximate age in days of a `git log --format=%cr` string
/// ("3 days ago", "10 minutes ago", "2 weeks ago").
fn relative_days(s: &str) -> Option<u64> {
    let mut words = s.split_whitespace();
    let n: u64 = words.next()?.parse().ok()?;
    let unit = words.next()?.trim_end_matches('s');
    Some(match unit {
        "second" | "minute" | "hour" => 0,
        "day" => n,
        "week" => n * 7,
        "month" => n * 30,
        "year" => n * 365,
        _ => return None,
    })
}

impl Theme {
    /// Paint one table cell of column `header`. Multi-part cells (STATUS's
    /// "2 mod, 1 untr") are painted per part. Unknown values ('-', '?', …)
    /// pass through unpainted.
    pub fn paint(&self, header: &str, cell: &str) -> String {
        match header {
            "PATH" => painted(self.path, cell),
            "BRANCH" => painted(self.branch, cell),
            "SIZE" => {
                let style = match du_bytes(cell) {
                    Some(b) if b > self.size_alert_bytes => self.size_alert,
                    Some(b) if b > self.size_warn_bytes => self.size_warn,
                    Some(_) => self.size,
                    None => return cell.to_string(),
                };
                painted(style, cell)
            }
            "STATUS" => cell
                .split(", ")
                .map(|part| {
                    if part == "clean" {
                        painted(self.status_clean, part)
                    } else if part.ends_with(" mod") {
                        painted(self.status_mod, part)
                    } else if part.ends_with(" untr") {
                        painted(self.status_untr, part)
                    } else {
                        part.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            "MERGED" => {
                if cell == "merged" {
                    painted(self.merged_ok, cell)
                } else if cell.starts_with('+') {
                    painted(self.merged_unmerged, cell)
                } else {
                    cell.to_string()
                }
            }
            "UPSTREAM" => {
                if cell == "ok" {
                    painted(self.upstream_ok, cell)
                } else if cell == "none" {
                    painted(self.upstream_none, cell)
                } else {
                    cell.to_string()
                }
            }
            "LAST" => {
                let style = match relative_days(cell) {
                    Some(d) if d <= self.last_fresh_days => self.last_fresh,
                    Some(d) if d < self.last_aging_days => self.last_aging,
                    Some(_) => self.last_old,
                    None => return cell.to_string(),
                };
                painted(style, cell)
            }
            _ => cell.to_string(),
        }
    }

    pub fn paint_header(&self, header: &str) -> String {
        painted(self.header, header)
    }

    /// One "[main]" / "[cwd]" marker: brackets and word painted separately.
    pub fn paint_marker(&self, name: &str) -> String {
        let word = match name {
            "main" => painted(self.marker_main, name),
            "cwd" => painted(self.marker_cwd, name),
            _ => name.to_string(),
        };
        format!(
            "{l}{word}{r}",
            l = painted(self.marker_brackets, "["),
            r = painted(self.marker_brackets, "]")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn du_bytes_parses_suffixes() {
        assert_eq!(du_bytes("512"), Some(512));
        assert_eq!(du_bytes("16K"), Some(16 << 10));
        assert_eq!(du_bytes("80M"), Some(80 << 20));
        assert_eq!(du_bytes("1.2G"), Some((1.2 * GIB as f64) as u64));
        assert_eq!(du_bytes("2T"), Some(2 * (1u64 << 40)));
        assert_eq!(du_bytes("?"), None);
    }

    #[test]
    fn relative_days_parses_git_cr() {
        assert_eq!(relative_days("10 seconds ago"), Some(0));
        assert_eq!(relative_days("5 hours ago"), Some(0));
        assert_eq!(relative_days("3 days ago"), Some(3));
        assert_eq!(relative_days("6 days ago"), Some(6));
        assert_eq!(relative_days("2 weeks ago"), Some(14));
        assert_eq!(relative_days("4 months ago"), Some(120));
        assert_eq!(relative_days("-"), None);
    }

    #[test]
    fn thresholds_pick_the_right_style() {
        let t = Theme::default();
        assert!(t.paint("SIZE", "80M").contains(&t.size.to_string()));
        assert!(t.paint("SIZE", "1.2G").contains(&t.size_warn.to_string()));
        assert!(t.paint("SIZE", "12G").contains(&t.size_alert.to_string()));
        assert!(
            t.paint("LAST", "2 days ago")
                .contains(&t.last_fresh.to_string())
        );
        assert!(
            t.paint("LAST", "5 days ago")
                .contains(&t.last_aging.to_string())
        );
        assert!(
            t.paint("LAST", "3 weeks ago")
                .contains(&t.last_old.to_string())
        );
    }

    #[test]
    fn status_parts_are_painted_separately() {
        let t = Theme::default();
        let out = t.paint("STATUS", "2 mod, 1 untr");
        assert!(out.contains(&t.status_mod.to_string()));
        assert!(out.contains(&t.status_untr.to_string()));
        assert_eq!(t.paint("STATUS", "-"), "-");
    }

    #[test]
    fn unknown_values_pass_through_unpainted() {
        let t = Theme::default();
        for (h, c) in [
            ("SIZE", "?"),
            ("MERGED", "?"),
            ("MERGED", "-"),
            ("UPSTREAM", "ahead 2"),
            ("LAST", "-"),
        ] {
            assert_eq!(t.paint(h, c), c, "{h}/{c}");
        }
    }
}
