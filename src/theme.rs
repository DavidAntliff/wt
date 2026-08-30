//! Colour for `list`'s table. Every colour and threshold is a field of
//! [`Theme`], never hard-coded at the paint site — the palette is meant to be
//! tweaked (and later loaded from configuration).
//!
//! Widths are always computed from PLAIN cell text; painting wraps the text
//! after padding is decided, so escapes can never enter a width calculation.

use anstyle::{Ansi256Color, AnsiColor, Color, Effects, RgbColor, Style};

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

const GIB: u64 = 1 << 30;

/// The built-in defaults, written in the same style-spec grammar a future
/// config file will use (the slogs grammar: attributes and one colour per
/// spec, in any order).
impl Default for Theme {
    fn default() -> Self {
        let s = |spec: &str| parse_style(spec).expect("built-in style spec");
        Theme {
            header: s("cyan bold"),
            path: s("bright-white"),
            branch: s("white"),
            marker_brackets: s("bright-white"),
            marker_main: s("bright-yellow"),
            marker_cwd: s("bright-green"),
            size: s("bright-white"),
            size_warn: s("bright-yellow"),
            size_alert: s("bright-red"),
            size_warn_bytes: GIB,
            size_alert_bytes: 10 * GIB,
            status_clean: s("green"),
            status_mod: s("red"),
            status_untr: s("yellow"),
            merged_ok: s("green"),
            merged_unmerged: s("yellow"),
            upstream_ok: s("green"),
            upstream_none: s("white"),
            last_fresh: s("green"),
            last_aging: s("yellow"),
            last_old: s("red"),
            last_fresh_days: 3,
            last_aging_days: 7,
        }
    }
}

/// Parse a style specification: attributes and at most one colour, in any
/// order — e.g. "cyan bold", "bright-white", "196 italic", "#ff8700".
/// Same grammar as slogs, so a shared config vocabulary is possible.
pub fn parse_style(spec: &str) -> Result<Style, String> {
    let mut style = Style::new();
    let mut colour: Option<Color> = None;

    for token in spec.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        let effect = match lower.as_str() {
            "bold" => Some(Effects::BOLD),
            "dim" => Some(Effects::DIMMED),
            "italic" => Some(Effects::ITALIC),
            "underline" => Some(Effects::UNDERLINE),
            "reverse" => Some(Effects::INVERT),
            _ => None,
        };
        if let Some(effect) = effect {
            style = style.effects(style.get_effects() | effect);
            continue;
        }
        // An explicit request for the terminal's own foreground, so a built-in
        // default can be switched off without deleting the key.
        if lower == "default" {
            continue;
        }
        if colour.is_some() {
            return Err(format!("more than one colour in {spec:?}"));
        }
        colour = Some(parse_colour(&lower)?);
    }

    if let Some(colour) = colour {
        style = style.fg_color(Some(colour));
    }
    Ok(style)
}

// ponytail: no xterm palette names (MistyRose1, ...) yet — lift slogs'
// XTERM_NAMES table verbatim when the config file lands.
fn parse_colour(token: &str) -> Result<Color, String> {
    if let Some(ansi) = ansi_name(token) {
        return Ok(Color::Ansi(ansi));
    }
    if let Ok(index) = token.parse::<u8>() {
        return Ok(Color::Ansi256(Ansi256Color(index)));
    }
    if let Some(rgb) = parse_hex(token) {
        return Ok(Color::Rgb(rgb));
    }
    Err(format!(
        "{token:?} is not a colour; expected an ANSI name, 0-255, or #rrggbb"
    ))
}

fn ansi_name(token: &str) -> Option<AnsiColor> {
    Some(match token {
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Red,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Yellow,
        "blue" => AnsiColor::Blue,
        "magenta" => AnsiColor::Magenta,
        "cyan" => AnsiColor::Cyan,
        "white" => AnsiColor::White,
        "bright-black" => AnsiColor::BrightBlack,
        "bright-red" => AnsiColor::BrightRed,
        "bright-green" => AnsiColor::BrightGreen,
        "bright-yellow" => AnsiColor::BrightYellow,
        "bright-blue" => AnsiColor::BrightBlue,
        "bright-magenta" => AnsiColor::BrightMagenta,
        "bright-cyan" => AnsiColor::BrightCyan,
        "bright-white" => AnsiColor::BrightWhite,
        _ => return None,
    })
}

/// `#rrggbb`, or the `#rgb` shorthand.
fn parse_hex(token: &str) -> Option<RgbColor> {
    let digits = token.strip_prefix('#')?;
    let pair = |s: &str| u8::from_str_radix(s, 16).ok();
    match digits.len() {
        6 => Some(RgbColor(
            pair(&digits[0..2])?,
            pair(&digits[2..4])?,
            pair(&digits[4..6])?,
        )),
        // #abc means #aabbcc, so each digit is doubled rather than shifted.
        3 => {
            let d = |i: usize| pair(&digits[i..i + 1]).map(|v| v * 17);
            Some(RgbColor(d(0)?, d(1)?, d(2)?))
        }
        _ => None,
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
    fn parse_style_grammar_matches_slogs() {
        // Order-independent, attribute + one colour.
        assert_eq!(parse_style("cyan bold"), parse_style("bold cyan"));
        let s = parse_style("bright-white").unwrap();
        assert_eq!(s.get_fg_color(), Some(Color::Ansi(AnsiColor::BrightWhite)));
        // Bare 256-index and hex forms.
        assert_eq!(
            parse_style("196").unwrap().get_fg_color(),
            Some(Color::Ansi256(Ansi256Color(196)))
        );
        assert_eq!(
            parse_style("#ff8700").unwrap().get_fg_color(),
            Some(Color::Rgb(RgbColor(0xff, 0x87, 0x00)))
        );
        assert_eq!(
            parse_style("#abc").unwrap().get_fg_color(),
            Some(Color::Rgb(RgbColor(0xaa, 0xbb, 0xcc)))
        );
        // "default" is a no-op; empty spec is the empty style.
        assert_eq!(parse_style("default").unwrap(), Style::new());
        assert_eq!(parse_style("").unwrap(), Style::new());
        // Errors: two colours, or a non-colour word.
        assert!(parse_style("red green").is_err());
        assert!(parse_style("chartreuse-ish").is_err());
    }

    #[test]
    fn default_theme_matches_the_agreed_palette() {
        let t = Theme::default();
        assert_eq!(t.header, parse_style("cyan bold").unwrap());
        assert_eq!(t.path, parse_style("bright-white").unwrap());
        assert_eq!(t.branch, parse_style("white").unwrap());
        assert_eq!(t.marker_brackets, parse_style("bright-white").unwrap());
        assert_eq!(t.marker_main, parse_style("bright-yellow").unwrap());
        assert_eq!(t.marker_cwd, parse_style("bright-green").unwrap());
        assert_eq!(t.size, parse_style("bright-white").unwrap());
        assert_eq!(t.size_warn, parse_style("bright-yellow").unwrap());
        assert_eq!(t.size_alert, parse_style("bright-red").unwrap());
        assert_eq!(t.merged_unmerged, parse_style("yellow").unwrap());
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
