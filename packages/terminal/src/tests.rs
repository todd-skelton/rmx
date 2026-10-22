//! Ports of Remix's `terminal` tests (index, ansi, env, styles, terminal), one test per case.
//!
//! Remix's `cursorTo(-1, NaN)` and fractional positions have no Rust counterpart, since
//! positions are `usize` and `isize`. `getDefaultEnvironment` returning `process.env` is
//! covered by `Terminal::new` reading the process environment when `env` is `None`. The
//! index.test.ts case (the entrypoint re-exports each module's APIs) does not apply: the
//! crate defines them at its root.

use std::sync::{Arc, Mutex};

use super::*;

#[test]
fn exposes_raw_style_and_control_sequences() {
    assert_eq!(ansi::RESET, "\x1b[0m");
    assert_eq!(Style::Red.open(), "\x1b[31m");
    assert_eq!(Style::GreenBright.open(), "\x1b[92m");
    assert_eq!(Style::BgBlue.open(), "\x1b[44m");
    assert_eq!((ansi::CLEAR_LINE, ansi::ERASE_DOWN, ansi::HIDE_CURSOR, ansi::SHOW_CURSOR), ("\x1b[2K", "\x1b[J", "\x1b[?25l", "\x1b[?25h"));
}

#[test]
fn generates_cursor_position_sequences() {
    assert_eq!(ansi::cursor_to(0, None), "\x1b[1G");
    assert_eq!(ansi::cursor_to(4, Some(2)), "\x1b[3;5H");
    assert_eq!(ansi::cursor_to(0, Some(0)), "\x1b[1;1H");
}

#[test]
fn generates_cursor_movement_sequences() {
    assert_eq!(ansi::move_cursor(2, 3), "\x1b[2C\x1b[3B");
    assert_eq!(ansi::move_cursor(-2, -3), "\x1b[2D\x1b[3A");
    assert_eq!(ansi::move_cursor(2, -3), "\x1b[2C\x1b[3A");
    assert_eq!(ansi::move_cursor(0, 0), "");
}

#[test]
fn strips_csi_and_osc_escape_sequences() {
    assert_eq!(strip_ansi("\x1b[31mError\x1b[0m"), "Error");
    assert_eq!(strip_ansi("\x1b[1;31mError\x1b[0m"), "Error");
    assert_eq!(strip_ansi("\x1b]8;;https://remix.run\x07Remix\x1b]8;;\x07"), "Remix");
    assert_eq!(strip_ansi("\x1b]8;;https://remix.run\x1b\\Remix\x1b]8;;\x1b\\"), "Remix");
    assert_eq!(strip_ansi("plain \x1b text"), "plain \x1b text", "a lone escape stays");
}

fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
}

#[test]
fn disables_colors_with_no_color() {
    assert!(!should_use_colors(env(&[("NO_COLOR", "")]), true));
}

#[test]
fn respects_force_color() {
    assert!(should_use_colors(env(&[("FORCE_COLOR", "1")]), false));
    assert!(should_use_colors(env(&[("FORCE_COLOR", "true")]), false));
    assert!(!should_use_colors(env(&[("FORCE_COLOR", "0")]), true));
    assert!(!should_use_colors(env(&[("FORCE_COLOR", "false")]), true));
}

#[test]
fn disables_colors_in_ci() {
    assert!(!should_use_colors(env(&[("CI", "true")]), true));
}

#[test]
fn disables_colors_for_dumb_terminals() {
    assert!(!should_use_colors(env(&[("TERM", "dumb")]), true));
}

#[test]
fn falls_back_to_tty_support() {
    assert!(should_use_colors(env(&[]), true));
    assert!(!should_use_colors(env(&[]), false));
}

#[test]
fn formats_values_with_every_named_style_helper() {
    let styles = Styles::new(true);
    let cases = [
        (Style::Bold, "\x1b[1mx\x1b[22m"),
        (Style::Dim, "\x1b[2mx\x1b[22m"),
        (Style::Italic, "\x1b[3mx\x1b[23m"),
        (Style::Underline, "\x1b[4mx\x1b[24m"),
        (Style::Inverse, "\x1b[7mx\x1b[27m"),
        (Style::Strikethrough, "\x1b[9mx\x1b[29m"),
        (Style::Overline, "\x1b[53mx\x1b[55m"),
        (Style::Black, "\x1b[30mx\x1b[39m"),
        (Style::Red, "\x1b[31mx\x1b[39m"),
        (Style::Green, "\x1b[32mx\x1b[39m"),
        (Style::Yellow, "\x1b[33mx\x1b[39m"),
        (Style::Blue, "\x1b[34mx\x1b[39m"),
        (Style::Magenta, "\x1b[35mx\x1b[39m"),
        (Style::Cyan, "\x1b[36mx\x1b[39m"),
        (Style::White, "\x1b[37mx\x1b[39m"),
        (Style::BlackBright, "\x1b[90mx\x1b[39m"),
        (Style::Gray, "\x1b[90mx\x1b[39m"),
        (Style::Grey, "\x1b[90mx\x1b[39m"),
        (Style::RedBright, "\x1b[91mx\x1b[39m"),
        (Style::GreenBright, "\x1b[92mx\x1b[39m"),
        (Style::YellowBright, "\x1b[93mx\x1b[39m"),
        (Style::BlueBright, "\x1b[94mx\x1b[39m"),
        (Style::MagentaBright, "\x1b[95mx\x1b[39m"),
        (Style::CyanBright, "\x1b[96mx\x1b[39m"),
        (Style::WhiteBright, "\x1b[97mx\x1b[39m"),
        (Style::BgBlack, "\x1b[40mx\x1b[49m"),
        (Style::BgRed, "\x1b[41mx\x1b[49m"),
        (Style::BgGreen, "\x1b[42mx\x1b[49m"),
        (Style::BgYellow, "\x1b[43mx\x1b[49m"),
        (Style::BgBlue, "\x1b[44mx\x1b[49m"),
        (Style::BgMagenta, "\x1b[45mx\x1b[49m"),
        (Style::BgCyan, "\x1b[46mx\x1b[49m"),
        (Style::BgWhite, "\x1b[47mx\x1b[49m"),
        (Style::BgBlackBright, "\x1b[100mx\x1b[49m"),
        (Style::BgGray, "\x1b[100mx\x1b[49m"),
        (Style::BgGrey, "\x1b[100mx\x1b[49m"),
        (Style::BgRedBright, "\x1b[101mx\x1b[49m"),
        (Style::BgGreenBright, "\x1b[102mx\x1b[49m"),
        (Style::BgYellowBright, "\x1b[103mx\x1b[49m"),
        (Style::BgBlueBright, "\x1b[104mx\x1b[49m"),
        (Style::BgMagentaBright, "\x1b[105mx\x1b[49m"),
        (Style::BgCyanBright, "\x1b[106mx\x1b[49m"),
        (Style::BgWhiteBright, "\x1b[107mx\x1b[49m"),
    ];
    assert_eq!(cases.len(), Style::ALL.len());
    for (style, expected) in cases {
        assert_eq!(styles.format("x", &[style]), expected, "{}", style.name());
        assert_eq!(style.name().parse::<Style>(), Ok(style));
    }
    assert!((styles.enabled(), styles.reset()) == (true, ansi::RESET));
    assert_eq!(styles.bg_blue_bright("x"), "\x1b[104mx\x1b[49m");
    assert_eq!(styles.strikethrough("x"), "\x1b[9mx\x1b[29m");
}

#[test]
fn formats_values_with_multiple_styles() {
    let styles = Styles::new(true);
    assert_eq!(styles.format("warning", &[]), "warning");
    assert_eq!(styles.format("warning", &[Style::Dim, Style::Yellow]), "\x1b[2m\x1b[33mwarning\x1b[39m\x1b[22m");
}

#[test]
fn uses_explicit_color_options_before_automatic_detection() {
    let options = |colors, env: &[(&str, &str)], tty| TerminalOptions {
        colors: Some(colors),
        env: Some(env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
        stdin_tty: Some(false),
        stdout: Some(Output::new(Vec::new(), false)),
        stderr: Some(Output::new(Vec::new(), false)),
        color_tty: Some(tty),
    };
    assert!(Terminal::new(options(true, &[("CI", "true"), ("NO_COLOR", ""), ("TERM", "dumb")], false)).styles.enabled());
    assert!(!Terminal::new(options(false, &[("FORCE_COLOR", "1")], true)).styles.enabled());
}

#[test]
fn restores_outer_styles_after_nested_styles_close() {
    let s = Styles::new(true);
    assert_eq!(s.red(&format!("Error: {} retrying", s.bold("fatal"))), "\x1b[31mError: \x1b[1mfatal\x1b[22m retrying\x1b[39m");
    assert_eq!(s.red(&format!("Error: {} retrying", s.green("fatal"))), "\x1b[31mError: \x1b[32mfatal\x1b[39m\x1b[31m retrying\x1b[39m");
    assert_eq!(s.bold(&format!("Error: {} retrying", s.dim("maybe"))), "\x1b[1mError: \x1b[2mmaybe\x1b[22m\x1b[1m retrying\x1b[22m");
    assert_eq!(s.bg_red(&format!("Error: {} retrying", s.bg_blue("fatal"))), "\x1b[41mError: \x1b[44mfatal\x1b[49m\x1b[41m retrying\x1b[49m");
}

#[test]
fn restores_outer_styles_after_nested_resets() {
    let s = Styles::new(true);
    assert_eq!(s.red(&format!("Error: {}retrying", ansi::RESET)), "\x1b[31mError: \x1b[0m\x1b[31mretrying\x1b[39m");
}

#[test]
fn passes_values_through_when_colors_are_disabled() {
    let s = Styles::new(false);
    assert_eq!((s.enabled(), s.reset()), (false, ""));
    assert_eq!(s.red("Error"), "Error");
    assert_eq!(s.format("warning", &[Style::Dim, Style::Yellow]), "warning");
}

#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Vec<u8>>>);

impl Write for Memory {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Memory {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn terminal(stdin_tty: bool, stdout_tty: bool, colors: Option<bool>, color_tty: Option<bool>) -> (Terminal, Memory, Memory) {
    let (out, err) = (Memory::default(), Memory::default());
    let t = Terminal::new(TerminalOptions { colors, env: Some(Vec::new()), stdin_tty: Some(stdin_tty), stdout: Some(Output::new(out.clone(), stdout_tty)), stderr: Some(Output::new(err.clone(), false)), color_tty });
    (t, out, err)
}

#[test]
fn writes_to_injected_output_streams() {
    let (mut t, out, err) = terminal(true, true, Some(true), None);
    assert!(t.is_tty() && t.is_interactive());
    let ok = t.styles.green("ok");
    t.write_line(&ok);
    let fail = t.styles.red("fail");
    t.error_line(&fail);
    t.clear_line();
    t.cursor_to(0, None);
    t.hide_cursor();
    t.show_cursor();
    assert_eq!(out.text(), "\x1b[32mok\x1b[39m\n\x1b[2K\x1b[1G\x1b[?25l\x1b[?25h");
    assert_eq!(err.text(), "\x1b[31mfail\x1b[39m\n");
}

#[test]
fn uses_the_configured_color_detection_stream() {
    let (mut t, out, _) = terminal(false, false, None, Some(true));
    assert!(t.styles.enabled());
    let ok = t.styles.green("ok");
    t.write_line(&ok);
    assert_eq!(out.text(), "\x1b[32mok\x1b[39m\n");
}

#[test]
fn reports_tty_and_interactivity_from_injected_streams() {
    assert!(terminal(true, true, None, None).0.is_interactive());
    assert!(!terminal(false, true, None, None).0.is_interactive());
    assert!(!terminal(true, false, None, None).0.is_tty());
}
