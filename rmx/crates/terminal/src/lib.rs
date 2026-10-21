//! Terminal output for rmx (Remix's `remix/terminal`): raw ANSI sequences ([`ansi`]),
//! [`strip_ansi`], color detection ([`should_use_colors`]), style helpers that restore the
//! outer style after a nested one closes ([`Styles`]), and a [`Terminal`] over injectable
//! output streams. No dependencies.

use std::io::{IsTerminal, Write};

/// Raw ANSI escape sequences (Remix's `ansi`).
pub mod ansi {
    /// Resets all styles.
    pub const RESET: &str = "\x1b[0m";
    /// Clears the current line.
    pub const CLEAR_LINE: &str = "\x1b[2K";
    /// Erases from the cursor to the end of the screen.
    pub const ERASE_DOWN: &str = "\x1b[J";
    pub const HIDE_CURSOR: &str = "\x1b[?25l";
    pub const SHOW_CURSOR: &str = "\x1b[?25h";

    /// Moves the cursor to a zero-based column, and row when given. Remix truncates and
    /// clamps numbers here; unsigned integers make that unnecessary.
    pub fn cursor_to(column: usize, row: Option<usize>) -> String {
        match row {
            None => format!("\x1b[{}G", column + 1),
            Some(row) => format!("\x1b[{};{}H", row + 1, column + 1),
        }
    }

    /// Moves the cursor by relative columns (right when positive) and rows (down when
    /// positive).
    pub fn move_cursor(columns: isize, rows: isize) -> String {
        let mut out = String::new();
        match columns {
            1.. => out.push_str(&format!("\x1b[{columns}C")),
            ..0 => out.push_str(&format!("\x1b[{}D", columns.unsigned_abs())),
            0 => {}
        }
        match rows {
            1.. => out.push_str(&format!("\x1b[{rows}B")),
            ..0 => out.push_str(&format!("\x1b[{}A", rows.unsigned_abs())),
            0 => {}
        }
        out
    }
}

/// A named terminal style (Remix's `TerminalStyleName`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Bold,
    Dim,
    Italic,
    Underline,
    Inverse,
    Strikethrough,
    Overline,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BlackBright,
    Gray,
    Grey,
    RedBright,
    GreenBright,
    YellowBright,
    BlueBright,
    MagentaBright,
    CyanBright,
    WhiteBright,
    BgBlack,
    BgRed,
    BgGreen,
    BgYellow,
    BgBlue,
    BgMagenta,
    BgCyan,
    BgWhite,
    BgBlackBright,
    BgGray,
    BgGrey,
    BgRedBright,
    BgGreenBright,
    BgYellowBright,
    BgBlueBright,
    BgMagentaBright,
    BgCyanBright,
    BgWhiteBright,
}

impl Style {
    /// Every style, in Remix's declaration order.
    pub const ALL: [Style; 43] = [Style::Bold, Style::Dim, Style::Italic, Style::Underline, Style::Inverse, Style::Strikethrough, Style::Overline, Style::Black, Style::Red, Style::Green, Style::Yellow, Style::Blue, Style::Magenta, Style::Cyan, Style::White, Style::BlackBright, Style::Gray, Style::Grey, Style::RedBright, Style::GreenBright, Style::YellowBright, Style::BlueBright, Style::MagentaBright, Style::CyanBright, Style::WhiteBright, Style::BgBlack, Style::BgRed, Style::BgGreen, Style::BgYellow, Style::BgBlue, Style::BgMagenta, Style::BgCyan, Style::BgWhite, Style::BgBlackBright, Style::BgGray, Style::BgGrey, Style::BgRedBright, Style::BgGreenBright, Style::BgYellowBright, Style::BgBlueBright, Style::BgMagentaBright, Style::BgCyanBright, Style::BgWhiteBright];

    /// The sequence that turns this style on (Remix's `ansiStyleCodes`).
    pub fn open(self) -> &'static str {
        match self {
            Style::Bold => "\x1b[1m",
            Style::Dim => "\x1b[2m",
            Style::Italic => "\x1b[3m",
            Style::Underline => "\x1b[4m",
            Style::Inverse => "\x1b[7m",
            Style::Strikethrough => "\x1b[9m",
            Style::Overline => "\x1b[53m",
            Style::Black => "\x1b[30m",
            Style::Red => "\x1b[31m",
            Style::Green => "\x1b[32m",
            Style::Yellow => "\x1b[33m",
            Style::Blue => "\x1b[34m",
            Style::Magenta => "\x1b[35m",
            Style::Cyan => "\x1b[36m",
            Style::White => "\x1b[37m",
            Style::BlackBright => "\x1b[90m",
            Style::Gray => "\x1b[90m",
            Style::Grey => "\x1b[90m",
            Style::RedBright => "\x1b[91m",
            Style::GreenBright => "\x1b[92m",
            Style::YellowBright => "\x1b[93m",
            Style::BlueBright => "\x1b[94m",
            Style::MagentaBright => "\x1b[95m",
            Style::CyanBright => "\x1b[96m",
            Style::WhiteBright => "\x1b[97m",
            Style::BgBlack => "\x1b[40m",
            Style::BgRed => "\x1b[41m",
            Style::BgGreen => "\x1b[42m",
            Style::BgYellow => "\x1b[43m",
            Style::BgBlue => "\x1b[44m",
            Style::BgMagenta => "\x1b[45m",
            Style::BgCyan => "\x1b[46m",
            Style::BgWhite => "\x1b[47m",
            Style::BgBlackBright => "\x1b[100m",
            Style::BgGray => "\x1b[100m",
            Style::BgGrey => "\x1b[100m",
            Style::BgRedBright => "\x1b[101m",
            Style::BgGreenBright => "\x1b[102m",
            Style::BgYellowBright => "\x1b[103m",
            Style::BgBlueBright => "\x1b[104m",
            Style::BgMagentaBright => "\x1b[105m",
            Style::BgCyanBright => "\x1b[106m",
            Style::BgWhiteBright => "\x1b[107m",
        }
    }

    /// The sequence that turns only this style off, leaving others active.
    pub fn close(self) -> &'static str {
        match self {
            Style::Bold | Style::Dim => "\x1b[22m",
            Style::Italic => "\x1b[23m",
            Style::Underline => "\x1b[24m",
            Style::Inverse => "\x1b[27m",
            Style::Strikethrough => "\x1b[29m",
            Style::Overline => "\x1b[55m",
            Style::Black | Style::Red | Style::Green | Style::Yellow | Style::Blue | Style::Magenta | Style::Cyan | Style::White | Style::BlackBright | Style::Gray | Style::Grey | Style::RedBright | Style::GreenBright | Style::YellowBright | Style::BlueBright | Style::MagentaBright | Style::CyanBright | Style::WhiteBright => "\x1b[39m",
            Style::BgBlack | Style::BgRed | Style::BgGreen | Style::BgYellow | Style::BgBlue | Style::BgMagenta | Style::BgCyan | Style::BgWhite | Style::BgBlackBright | Style::BgGray | Style::BgGrey | Style::BgRedBright | Style::BgGreenBright | Style::BgYellowBright | Style::BgBlueBright | Style::BgMagentaBright | Style::BgCyanBright | Style::BgWhiteBright => "\x1b[49m",
        }
    }

    /// The Remix name (`"bgBlueBright"`).
    pub fn name(self) -> &'static str {
        match self {
            Style::Bold => "bold",
            Style::Dim => "dim",
            Style::Italic => "italic",
            Style::Underline => "underline",
            Style::Inverse => "inverse",
            Style::Strikethrough => "strikethrough",
            Style::Overline => "overline",
            Style::Black => "black",
            Style::Red => "red",
            Style::Green => "green",
            Style::Yellow => "yellow",
            Style::Blue => "blue",
            Style::Magenta => "magenta",
            Style::Cyan => "cyan",
            Style::White => "white",
            Style::BlackBright => "blackBright",
            Style::Gray => "gray",
            Style::Grey => "grey",
            Style::RedBright => "redBright",
            Style::GreenBright => "greenBright",
            Style::YellowBright => "yellowBright",
            Style::BlueBright => "blueBright",
            Style::MagentaBright => "magentaBright",
            Style::CyanBright => "cyanBright",
            Style::WhiteBright => "whiteBright",
            Style::BgBlack => "bgBlack",
            Style::BgRed => "bgRed",
            Style::BgGreen => "bgGreen",
            Style::BgYellow => "bgYellow",
            Style::BgBlue => "bgBlue",
            Style::BgMagenta => "bgMagenta",
            Style::BgCyan => "bgCyan",
            Style::BgWhite => "bgWhite",
            Style::BgBlackBright => "bgBlackBright",
            Style::BgGray => "bgGray",
            Style::BgGrey => "bgGrey",
            Style::BgRedBright => "bgRedBright",
            Style::BgGreenBright => "bgGreenBright",
            Style::BgYellowBright => "bgYellowBright",
            Style::BgBlueBright => "bgBlueBright",
            Style::BgMagentaBright => "bgMagentaBright",
            Style::BgCyanBright => "bgCyanBright",
            Style::BgWhiteBright => "bgWhiteBright",
        }
    }
}

impl std::str::FromStr for Style {
    type Err = String;

    fn from_str(name: &str) -> Result<Style, String> {
        Style::ALL.into_iter().find(|s| s.name() == name).ok_or_else(|| format!("unknown terminal style {name:?}"))
    }
}

/// Removes ANSI escape sequences: CSI (`ESC [ ... final`) and OSC (`ESC ] ... BEL` or
/// `ESC ] ... ESC \\`), such as hyperlinks.
pub fn strip_ansi(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(i) = rest.find('\x1b') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let skip = match after.as_bytes().first() {
            Some(b'[') => csi_len(&after[1..]).map(|n| n + 1),
            Some(b']') => osc_len(&after[1..]).map(|n| n + 1),
            _ => None,
        };
        match skip {
            Some(n) => rest = &after[n..],
            None => {
                out.push('\x1b');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The length of a CSI body: parameter bytes `0-?`, intermediate bytes ` -/`, a final
/// byte `@-~`.
fn csi_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (0x30..=0x3f).contains(&b[i]) {
        i += 1;
    }
    while i < b.len() && (0x20..=0x2f).contains(&b[i]) {
        i += 1;
    }
    (i < b.len() && (0x40..=0x7e).contains(&b[i])).then_some(i + 1)
}

/// The length of an OSC body through its terminator (BEL or `ESC \\`).
fn osc_len(s: &str) -> Option<usize> {
    let bel = s.find('\x07').map(|i| i + 1);
    let st = s.find("\x1b\\").map(|i| i + 2);
    match (bel, st) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Whether to emit colors (Remix's `shouldUseColors`): `NO_COLOR` turns them off,
/// `FORCE_COLOR` on unless it is `0` or `false`, `CI=true` and `TERM=dumb` off,
/// otherwise whether the output is a terminal. `env` reads a variable.
pub fn should_use_colors(env: impl Fn(&str) -> Option<String>, is_tty: bool) -> bool {
    if env("NO_COLOR").is_some() {
        return false;
    }
    if let Some(force) = env("FORCE_COLOR") {
        let value = force.to_ascii_lowercase();
        return value != "0" && value != "false";
    }
    if env("CI").as_deref() == Some("true") || env("TERM").as_deref() == Some("dumb") {
        return false;
    }
    is_tty
}

/// [`should_use_colors`] for this process's environment and stdout.
pub fn stdout_colors() -> bool {
    should_use_colors(|name| std::env::var(name).ok(), std::io::stdout().is_terminal())
}

/// Style helpers that emit ANSI sequences or pass text through (Remix's `createStyles`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Styles {
    enabled: bool,
}

macro_rules! style_methods {
    ($($method:ident => $style:ident),* $(,)?) => {
        $(
            pub fn $method(&self, value: &str) -> String {
                self.format(value, &[Style::$style])
            }
        )*
    };
}

impl Styles {
    /// Styles on or off explicitly (Remix's `colors` option).
    pub fn new(enabled: bool) -> Styles {
        Styles { enabled }
    }

    /// Styles detected for stdout ([`stdout_colors`]).
    pub fn detect() -> Styles {
        Styles::new(stdout_colors())
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// [`ansi::RESET`] when enabled, otherwise empty.
    pub fn reset(&self) -> &'static str {
        if self.enabled {
            ansi::RESET
        } else {
            ""
        }
    }

    /// `value` with `styles`, outermost first. Inside `value`, a nested close or reset
    /// re-opens each style so the rest of the text keeps it.
    pub fn format(&self, value: &str, styles: &[Style]) -> String {
        if !self.enabled {
            return value.to_string();
        }
        styles.iter().rev().fold(value.to_string(), |text, style| {
            let restored = text
                .replace(style.close(), &format!("{}{}", style.close(), style.open()))
                .replace(ansi::RESET, &format!("{}{}", ansi::RESET, style.open()));
            format!("{}{restored}{}", style.open(), style.close())
        })
    }

    style_methods! {
        bold => Bold,
        dim => Dim,
        italic => Italic,
        underline => Underline,
        inverse => Inverse,
        strikethrough => Strikethrough,
        overline => Overline,
        black => Black,
        red => Red,
        green => Green,
        yellow => Yellow,
        blue => Blue,
        magenta => Magenta,
        cyan => Cyan,
        white => White,
        black_bright => BlackBright,
        gray => Gray,
        grey => Grey,
        red_bright => RedBright,
        green_bright => GreenBright,
        yellow_bright => YellowBright,
        blue_bright => BlueBright,
        magenta_bright => MagentaBright,
        cyan_bright => CyanBright,
        white_bright => WhiteBright,
        bg_black => BgBlack,
        bg_red => BgRed,
        bg_green => BgGreen,
        bg_yellow => BgYellow,
        bg_blue => BgBlue,
        bg_magenta => BgMagenta,
        bg_cyan => BgCyan,
        bg_white => BgWhite,
        bg_black_bright => BgBlackBright,
        bg_gray => BgGray,
        bg_grey => BgGrey,
        bg_red_bright => BgRedBright,
        bg_green_bright => BgGreenBright,
        bg_yellow_bright => BgYellowBright,
        bg_blue_bright => BgBlueBright,
        bg_magenta_bright => BgMagentaBright,
        bg_cyan_bright => BgCyanBright,
        bg_white_bright => BgWhiteBright,
    }
}

/// Where a [`Terminal`] writes, and whether it is a TTY.
pub struct Output {
    writer: Box<dyn Write + Send>,
    is_tty: bool,
}

impl Output {
    pub fn new(writer: impl Write + Send + 'static, is_tty: bool) -> Output {
        Output { writer: Box::new(writer), is_tty }
    }

    pub fn stdout() -> Output {
        Output::new(std::io::stdout(), std::io::stdout().is_terminal())
    }

    pub fn stderr() -> Output {
        Output::new(std::io::stderr(), std::io::stderr().is_terminal())
    }
}

/// Options for [`Terminal::new`] (Remix's `TerminalOptions`). Unset fields use the process:
/// its stdin, stdout and stderr and its environment.
#[derive(Default)]
pub struct TerminalOptions {
    /// Colors on or off, skipping detection.
    pub colors: Option<bool>,
    /// Environment variables for detection; `None` reads the process environment.
    pub env: Option<Vec<(String, String)>>,
    /// Whether stdin is a TTY.
    pub stdin_tty: Option<bool>,
    pub stdout: Option<Output>,
    pub stderr: Option<Output>,
    /// The TTY flag color detection uses instead of stdout's (Remix's `stream`).
    pub color_tty: Option<bool>,
}

/// A terminal to write styled output to (Remix's `createTerminal`).
pub struct Terminal {
    pub styles: Styles,
    stdout: Output,
    stderr: Output,
    stdin_tty: bool,
}

impl Terminal {
    pub fn new(options: TerminalOptions) -> Terminal {
        let stdout = options.stdout.unwrap_or_else(Output::stdout);
        let stderr = options.stderr.unwrap_or_else(Output::stderr);
        let stdin_tty = options.stdin_tty.unwrap_or_else(|| std::io::stdin().is_terminal());
        let enabled = options.colors.unwrap_or_else(|| {
            let tty = options.color_tty.unwrap_or(stdout.is_tty);
            match &options.env {
                Some(vars) => should_use_colors(|name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()), tty),
                None => should_use_colors(|name| std::env::var(name).ok(), tty),
            }
        });
        Terminal { styles: Styles::new(enabled), stdout, stderr, stdin_tty }
    }

    /// The process's terminal.
    pub fn stdio() -> Terminal {
        Terminal::new(TerminalOptions::default())
    }

    /// Whether stdout is a TTY.
    pub fn is_tty(&self) -> bool {
        self.stdout.is_tty
    }

    /// Whether both stdin and stdout are TTYs, so prompts can be answered.
    pub fn is_interactive(&self) -> bool {
        self.stdin_tty && self.stdout.is_tty
    }

    pub fn write(&mut self, value: &str) {
        let _ = self.stdout.writer.write_all(value.as_bytes());
        let _ = self.stdout.writer.flush();
    }

    pub fn write_line(&mut self, value: &str) {
        self.write(&format!("{value}\n"));
    }

    pub fn error(&mut self, value: &str) {
        let _ = self.stderr.writer.write_all(value.as_bytes());
        let _ = self.stderr.writer.flush();
    }

    pub fn error_line(&mut self, value: &str) {
        self.error(&format!("{value}\n"));
    }

    pub fn clear_line(&mut self) {
        self.write(ansi::CLEAR_LINE);
    }

    pub fn erase_down(&mut self) {
        self.write(ansi::ERASE_DOWN);
    }

    pub fn cursor_to(&mut self, column: usize, row: Option<usize>) {
        self.write(&ansi::cursor_to(column, row));
    }

    pub fn move_cursor(&mut self, columns: isize, rows: isize) {
        self.write(&ansi::move_cursor(columns, rows));
    }

    pub fn hide_cursor(&mut self) {
        self.write(ansi::HIDE_CURSOR);
    }

    pub fn show_cursor(&mut self) {
        self.write(ansi::SHOW_CURSOR);
    }
}

#[cfg(test)]
mod tests;
