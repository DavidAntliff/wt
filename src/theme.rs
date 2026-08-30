//! Colour for `list`'s table. Every colour and threshold is a field of
//! [`Theme`], never hard-coded at the paint site. The palette itself lives in
//! `config::DEFAULT_CONFIG` (colours come from a config file and nowhere
//! else); [`Theme::default`] is entirely unstyled.
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
    /// Whether to colour, resolved as cargo (and slogs) do:
    /// `CLICOLOR_FORCE` beats everything, then `NO_COLOR`, then whether
    /// stdout is a terminal that claims to support colour.
    pub fn enabled(self) -> bool {
        use std::io::IsTerminal;
        self.resolve(std::io::stdout().is_terminal())
    }

    /// The same rule for stderr narration, whose terminal is stderr (stdout is
    /// usually captured by the `wt` shell function).
    pub fn enabled_stderr(self) -> bool {
        use std::io::IsTerminal;
        self.resolve(std::io::stderr().is_terminal())
    }

    fn resolve(self, is_terminal: bool) -> bool {
        match self {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => {
                if anstyle_query::clicolor_force() {
                    return true;
                }
                if anstyle_query::no_color() {
                    return false;
                }
                is_terminal && anstyle_query::term_supports_color()
            }
        }
    }
}

/// The palette and the thresholds that pick between its entries.
#[derive(Debug, Clone, PartialEq)]
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

/// Entirely unstyled, with the standard thresholds. The actual default
/// palette comes from parsing `config::DEFAULT_CONFIG` (see `config::defaults`)
/// so the template and the built-in colours cannot drift apart.
impl Default for Theme {
    fn default() -> Self {
        Theme {
            header: Style::new(),
            path: Style::new(),
            branch: Style::new(),
            marker_brackets: Style::new(),
            marker_main: Style::new(),
            marker_cwd: Style::new(),
            size: Style::new(),
            size_warn: Style::new(),
            size_alert: Style::new(),
            size_warn_bytes: GIB,
            size_alert_bytes: 10 * GIB,
            status_clean: Style::new(),
            status_mod: Style::new(),
            status_untr: Style::new(),
            merged_ok: Style::new(),
            merged_unmerged: Style::new(),
            upstream_ok: Style::new(),
            upstream_none: Style::new(),
            last_fresh: Style::new(),
            last_aging: Style::new(),
            last_old: Style::new(),
            last_fresh_days: 3,
            last_aging_days: 7,
        }
    }
}

impl Theme {
    /// The mutable style behind a config key. The key vocabulary is defined by
    /// `config::KEYS`; a test there asserts the two stay in sync.
    pub fn slot(&mut self, key: &str) -> Option<&mut Style> {
        Some(match key {
            "header" => &mut self.header,
            "path" => &mut self.path,
            "branch" => &mut self.branch,
            "marker-brackets" => &mut self.marker_brackets,
            "marker-main" => &mut self.marker_main,
            "marker-cwd" => &mut self.marker_cwd,
            "size" => &mut self.size,
            "size-warn" => &mut self.size_warn,
            "size-alert" => &mut self.size_alert,
            "status-clean" => &mut self.status_clean,
            "status-modified" => &mut self.status_mod,
            "status-untracked" => &mut self.status_untr,
            "merged" => &mut self.merged_ok,
            "unmerged" => &mut self.merged_unmerged,
            "upstream-ok" => &mut self.upstream_ok,
            "upstream-none" => &mut self.upstream_none,
            "last-fresh" => &mut self.last_fresh,
            "last-aging" => &mut self.last_aging,
            "last-old" => &mut self.last_old,
            _ => return None,
        })
    }

    /// Replace every 24-bit colour with the nearest 256-palette entry.
    ///
    /// Called when the terminal does not advertise truecolor, because
    /// `anstyle` renders whatever it is given rather than degrading.
    pub fn approximate_rgb(&mut self) {
        for key in crate::config::KEYS {
            let slot = self.slot(key).expect("KEYS and slot() are in sync");
            *slot = downgrade(*slot);
        }
    }
}

fn downgrade(style: Style) -> Style {
    match style.get_fg_color() {
        Some(Color::Rgb(RgbColor(r, g, b))) => {
            style.fg_color(Some(Color::Ansi256(Ansi256Color(rgb_to_ansi256(r, g, b)))))
        }
        _ => style,
    }
}

/// Nearest 256-palette entry to a 24-bit colour (ported from slogs).
///
/// Considers both the 6×6×6 colour cube and the 24-step greyscale ramp and
/// takes whichever is closer, which matters for near-grey colours that the
/// cube renders badly.
fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    /// The six levels the cube samples each channel at.
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    let nearest_level = |v: u8| -> usize {
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, l)| l.abs_diff(v))
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let distance = |a: (u8, u8, u8), b: (u8, u8, u8)| -> u32 {
        let d = |x: u8, y: u8| (x.abs_diff(y) as u32).pow(2);
        d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
    };

    let (ri, gi, bi) = (nearest_level(r), nearest_level(g), nearest_level(b));
    let cube_rgb = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let cube_index = 16 + 36 * ri + 6 * gi + bi;

    // Greyscale ramp: 232..=255 covers 8, 18, 28, ... 238.
    let average = ((r as u16 + g as u16 + b as u16) / 3) as u8;
    let step = ((average.saturating_sub(8) as u16 * 24) / 247).min(23) as u8;
    let grey_level = 8 + step * 10;
    let grey_index = 232 + step as usize;

    if distance((r, g, b), (grey_level, grey_level, grey_level)) < distance((r, g, b), cube_rgb) {
        grey_index as u8
    } else {
        cube_index as u8
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
/// ("3 days ago", "10 minutes ago", "2 years, 8 months ago" — combined forms
/// keep only the leading quantity, which is precise enough for the age bands).
fn relative_days(s: &str) -> Option<u64> {
    let mut words = s.split_whitespace();
    let n: u64 = words.next()?.parse().ok()?;
    let unit = words.next()?.trim_end_matches(',').trim_end_matches('s');
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
        // Combined forms: only the leading quantity counts.
        assert_eq!(relative_days("1 year, 2 months ago"), Some(365));
        assert_eq!(relative_days("2 years, 8 months ago"), Some(730));
        assert_eq!(relative_days("-"), None);
    }

    #[test]
    fn thresholds_pick_the_right_style() {
        let t = crate::config::defaults();
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
        let t = crate::config::defaults();
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
    fn rgb_downgrade_hits_the_nearest_palette_entry() {
        // Pure grey lands on the greyscale ramp, a saturated colour on the cube.
        assert_eq!(rgb_to_ansi256(0xff, 0x87, 0x00), 208); // DarkOrange
        let mut t = Theme::default();
        *t.slot("path").unwrap() = parse_style("#ff8700").unwrap();
        *t.slot("branch").unwrap() = parse_style("green").unwrap();
        t.approximate_rgb();
        assert_eq!(
            t.path.get_fg_color(),
            Some(Color::Ansi256(Ansi256Color(208)))
        );
        // Non-RGB colours are untouched.
        assert_eq!(t.branch, parse_style("green").unwrap());
    }

    #[test]
    fn unknown_values_pass_through_unpainted() {
        let t = crate::config::defaults();
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
