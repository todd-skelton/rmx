//! The `rmx` command (Remix's `remix` CLI): `new`, `dev`, `doctor`, `completion`, `routes`, `assets`, `test`,
//! `version` and `help`. Install it with `cargo install --path packages/rmx`.
//!
//! `new` writes the starter app (`template/`, like Remix's `template`) as a standalone
//! crate. `routes` and `assets` need the app's own route map and asset server, so they run
//! the app binary (`cargo run --quiet -- routes`), whose `main` handles them with
//! [`routes_command`](crate::router::routes::routes_command) and
//! [`assets_command`](crate::assets::assets_command). `test` runs `cargo test`.

use std::fmt;
use std::io::IsTerminal;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use rmx_terminal::Styles;

pub mod completion;
pub mod dev;
pub mod doctor;
mod status;

/// What a command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    /// Print to stdout and exit 0 (version, completion).
    Print(String),
    /// Print a help text to stdout and exit 0, colored on a terminal (Remix `formatHelpText`).
    Help(&'static str),
    /// Run a program in the current directory and exit with its status.
    Exec(Vec<String>),
    /// Create an app.
    New(NewOptions),
    /// Check the toolchain and the app (`--json`, `--strict`).
    Doctor { json: bool, strict: bool, fix: bool },
    /// Build, run and rebuild the app on changes; the arguments go to the app.
    Dev(Vec<String>),
}

/// Options for `rmx new`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewOptions {
    pub target_dir: PathBuf,
    /// `--app-name`: the display name; the directory name when absent.
    pub app_name: Option<String>,
    /// `--force`: write into a non-empty directory.
    pub force: bool,
}

/// A CLI error, rendered like Remix's (`Error [RMX_...] Title`, the message, a fix).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError {
    pub code: &'static str,
    pub title: &'static str,
    pub message: String,
    pub fix: Option<&'static str>,
    /// Print the command's help after the error (usage errors).
    pub show_help: bool,
}

impl CliError {
    fn usage(code: &'static str, title: &'static str, fix: &'static str, message: String) -> CliError {
        CliError { code, title, message, fix: Some(fix), show_help: true }
    }

    fn unknown_argument(arg: &str) -> CliError {
        Self::usage("RMX_UNKNOWN_ARGUMENT", "Unknown argument", "Run the command with --help to see supported arguments.", format!("Unknown argument: {arg}"))
    }

    fn unexpected_extra_argument(arg: &str) -> CliError {
        Self::usage(
            "RMX_UNEXPECTED_EXTRA_ARGUMENT",
            "Unexpected extra argument",
            "Remove the extra argument or run the command with --help.",
            format!("Unexpected extra argument: {arg}"),
        )
    }

    fn missing_option_value(flag: &str) -> CliError {
        Self::usage("RMX_MISSING_OPTION_VALUE", "Missing option value", "Pass a value immediately after the option.", format!("{flag} requires a value."))
    }

    fn unknown_command(command: &str) -> CliError {
        Self::usage("RMX_UNKNOWN_COMMAND", "Unknown command", "Run `rmx help` to see available commands.", format!("Unknown command: {command}"))
    }

    fn internal(message: String) -> CliError {
        CliError { code: "RMX_INTERNAL_ERROR", title: "Unexpected rmx CLI error", message, fix: None, show_help: false }
    }

    /// The error text, followed by `help` for usage errors.
    pub fn render(&self, help: &str) -> String {
        let mut out = format!("Error [{}] {}\n{}\n", self.code, self.title, self.message);
        if let Some(fix) = self.fix {
            out.push_str(&format!("\nTry:\n  {fix}\n"));
        }
        if self.show_help {
            out.push('\n');
            out.push_str(help);
        }
        out
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

pub const HELP: &str = "\
Usage:
  rmx <command> [options]

Commands:
  assets [command]  List or inspect browser-reachable assets
  completion        Print shell completion scripts for rmx
  dev [-- args]     Build and run the app, rebuilding and reloading on changes
  doctor            Check project health for the current project
  help [command]    Show help for rmx commands
  new <name>        Create a new rmx project
  routes            Show the route tree for the current project
  test [filter]     Run tests for the current project
  version           Show the current rmx version

Options:
  -h, --help     Show help
  --no-color     Disable ANSI color output
  -v, --version  Show version

Examples:
  rmx assets
  rmx completion bash
  rmx dev
  rmx doctor
  rmx help
  rmx new my-rmx-app
  rmx new my-rmx-app --app-name \"My rmx App\"
  rmx routes
  rmx test
  rmx version
";

const NEW_HELP: &str = "\
Usage:
  rmx new <target-dir> [--app-name <name>] [--force]

Create a new rmx project in the target directory.

Examples:
  rmx new ./my-rmx-app
  rmx new ./my-rmx-app --app-name \"My rmx App\"
  rmx new ./my-rmx-app --force
";

const VERSION_HELP: &str = "\
Usage:
  rmx version

Show the current rmx version.

Examples:
  rmx version
  rmx --version
";

const ROUTES_HELP: &str = "\
Usage:
  rmx routes [--json | --table] [--no-headers]

Show the route tree for the current project (runs `cargo run --quiet -- routes`).
";

const ASSETS_HELP: &str = "\
Usage:
  rmx assets
  rmx assets inspect <url-or-file>

List or inspect browser-reachable assets (runs `cargo run --quiet -- assets`).
";

const DOCTOR_HELP: &str = "\
Usage:
  rmx doctor [--json] [--strict] [--fix] [--no-color]

Check the toolchain and rmx app conventions for the current project.

Options:
  --json       Print doctor findings as JSON
  --strict     Exit with status 1 when warning-level findings are present
  --no-strict  Do not exit with status 1 when warning-level findings are present
  --fix        Apply available low-risk fixes

Examples:
  rmx doctor
  rmx doctor --json
  rmx doctor --strict
  rmx doctor --fix
";

const DEV_HELP: &str = "\
Usage:
  rmx dev [-- <app args>]

Build the client and the server, run the app, and rebuild and restart it when src/, app/,
public/ or Cargo.toml change. Pages reload when the app installs
rmx::middleware::live_reload. Set RMX_WASM_BINDGEN to use a wasm-bindgen that is not on PATH.

Examples:
  rmx dev
  rmx dev -- --port 4000
";

const COMPLETION_HELP: &str = "\
Usage:
  rmx completion <bash|zsh>

Print a shell completion script for rmx.

Examples:
  rmx completion bash >> ~/.bashrc
  rmx completion zsh >> ~/.zshrc
";

const TEST_HELP: &str = "\
Usage:
  rmx test [filter] [cargo test options] [-- <test binary options>]

Run tests for the current project. The arguments go to `cargo test` unchanged, and
everything after `--` (`-h` and `--help` too) goes to the test binaries.

Examples:
  rmx test
  rmx test frame
  rmx test -p my-app -- --nocapture
";

/// The help text for `command`, or the root help.
pub fn help_for(command: Option<&str>) -> Result<&'static str, CliError> {
    Ok(match command {
        None => HELP,
        Some("new") => NEW_HELP,
        Some("version") => VERSION_HELP,
        Some("routes") => ROUTES_HELP,
        Some("assets") => ASSETS_HELP,
        Some("test") => TEST_HELP,
        Some("doctor") => DOCTOR_HELP,
        Some("dev") => DEV_HELP,
        Some("completion") => COMPLETION_HELP,
        Some("help") => HELP,
        Some(other) => return Err(CliError::unknown_command(other)),
    })
}

/// The rmx version this CLI belongs to.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Options that apply to every command, taken out of the command line before it is
/// parsed (Remix's `extractGlobalOptions`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalOptions {
    /// `--no-color`: no ANSI styling, here or in the cargo commands `rmx` runs.
    pub no_color: bool,
}

/// Split the global options from the rest of a command line. Arguments from the first
/// `--` on are left alone.
pub fn global_options(args: &[String]) -> (Vec<String>, GlobalOptions) {
    let mut rest = Vec::with_capacity(args.len());
    let mut options = GlobalOptions::default();
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--" => {
                rest.extend_from_slice(&args[i..]);
                break;
            }
            "--no-color" => options.no_color = true,
            _ => rest.push(arg.clone()),
        }
    }
    (rest, options)
}

/// Parse a command line (without the program name). Global options are accepted and
/// ignored here; [`run`] applies them.
pub fn parse(args: &[String]) -> Result<Invocation, CliError> {
    let (args, _) = global_options(args);
    let mut args: Vec<&str> = args.iter().map(String::as_str).skip_while(|a| *a == "--").collect();
    if args.is_empty() || matches!(args[0], "-h" | "--help") {
        return Ok(Invocation::Help(HELP));
    }
    let command = args.remove(0);
    let wants_help = args.iter().any(|a| matches!(*a, "-h" | "--help"));
    match command {
        "help" => match args.as_slice() {
            [] => Ok(Invocation::Help(HELP)),
            [command] => Ok(Invocation::Help(help_for(Some(command))?)),
            [_, extra, ..] => Err(CliError::unexpected_extra_argument(extra)),
        },
        "-v" | "--version" | "version" => {
            if wants_help && command == "version" {
                return Ok(Invocation::Help(VERSION_HELP));
            }
            match args.first() {
                Some(arg) if arg.starts_with('-') => Err(CliError::unknown_argument(arg)),
                Some(arg) => Err(CliError::unexpected_extra_argument(arg)),
                None => Ok(Invocation::Print(format!("{}\n", version()))),
            }
        }
        "new" => {
            if args.is_empty() || wants_help {
                return Ok(Invocation::Help(NEW_HELP));
            }
            parse_new(&args).map(Invocation::New)
        }
        "routes" if wants_help => Ok(Invocation::Help(ROUTES_HELP)),
        "assets" if wants_help => Ok(Invocation::Help(ASSETS_HELP)),
        "routes" | "assets" => {
            let mut argv: Vec<String> = ["cargo", "run", "--quiet", "--", command].map(String::from).to_vec();
            argv.extend(args.iter().map(|a| a.to_string()));
            Ok(Invocation::Exec(argv))
        }
        "completion" => match args.as_slice() {
            // The scripts' query: `completion -- <index> <words...>`.
            ["--", index, words @ ..] => {
                let index = index.parse().unwrap_or(usize::MAX);
                let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
                Ok(Invocation::Print(completion::complete(&words, index).render()))
            }
            [] => Ok(Invocation::Help(COMPLETION_HELP)),
            _ if wants_help => Ok(Invocation::Help(COMPLETION_HELP)),
            [shell] if completion::SHELLS.contains(shell) => Ok(Invocation::Print(completion::script().into())),
            [shell] if shell.starts_with('-') => Err(CliError::unknown_argument(shell)),
            [shell] => Err(CliError::usage(
                "RMX_UNKNOWN_COMPLETION_SHELL",
                "Unknown completion shell",
                "Use `rmx completion bash` or `rmx completion zsh`.",
                format!("Unknown completion shell: {shell}"),
            )),
            [_, extra, ..] => Err(CliError::unexpected_extra_argument(extra)),
        },
        "dev" => {
            let (before, after) = match args.iter().position(|a| *a == "--") {
                Some(i) => (&args[..i], &args[i + 1..]),
                None => (&args[..], &[][..]),
            };
            if before.iter().any(|a| matches!(*a, "-h" | "--help")) {
                return Ok(Invocation::Help(DEV_HELP));
            }
            if let Some(arg) = before.first() {
                return Err(if arg.starts_with('-') { CliError::unknown_argument(arg) } else { CliError::unexpected_extra_argument(arg) });
            }
            Ok(Invocation::Dev(after.iter().map(|a| a.to_string()).collect()))
        }
        "doctor" => {
            if wants_help {
                return Ok(Invocation::Help(DOCTOR_HELP));
            }
            let (mut json, mut strict, mut fix) = (false, false, false);
            for arg in &args {
                match *arg {
                    "--fix" => fix = true,
                    "--json" => json = true,
                    "--strict" => strict = true,
                    "--no-strict" => strict = false,
                    other if other.starts_with('-') => return Err(CliError::unknown_argument(other)),
                    other => return Err(CliError::unexpected_extra_argument(other)),
                }
            }
            Ok(Invocation::Doctor { json, strict, fix })
        }
        "test" => {
            // After a bare `--` the arguments belong to the test binaries (Remix `wantsHelp`).
            if args.iter().take_while(|a| **a != "--").any(|a| matches!(*a, "-h" | "--help")) {
                return Ok(Invocation::Help(TEST_HELP));
            }
            let mut argv: Vec<String> = vec!["cargo".into(), "test".into()];
            argv.extend(args.iter().map(|a| a.to_string()));
            Ok(Invocation::Exec(argv))
        }
        other => Err(CliError::unknown_command(other)),
    }
}

fn parse_new(args: &[&str]) -> Result<NewOptions, CliError> {
    let mut options = NewOptions::default();
    let mut target = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with('-') => (flag, Some(value)),
            _ => (arg, None),
        };
        match flag {
            "--force" if inline.is_none() => options.force = true,
            "--app-name" => {
                let value = match inline {
                    Some("") => return Err(CliError::missing_option_value(flag)),
                    Some(value) => value,
                    None => {
                        i += 1;
                        match args.get(i) {
                            Some(next) if !next.is_empty() && !next.starts_with('-') => next,
                            _ => return Err(CliError::missing_option_value(flag)),
                        }
                    }
                };
                options.app_name = Some(value.to_string());
            }
            _ if arg.starts_with('-') => return Err(CliError::unknown_argument(arg)),
            _ if target.is_some() => return Err(CliError::unexpected_extra_argument(arg)),
            _ => target = Some(arg),
        }
        i += 1;
    }
    let Some(target) = target else {
        return Err(CliError::usage(
            "RMX_MISSING_TARGET_DIRECTORY",
            "Missing target directory",
            "Pass a target directory, for example `rmx new my-rmx-app`.",
            "A target directory is required.".into(),
        ));
    };
    options.target_dir = PathBuf::from(target);
    Ok(options)
}

/// Run the `rmx` command line and return the exit code: help and errors go to stdout and
/// stderr, `new` reports its steps on stderr, and `routes`, `assets` and `test` hand over to
/// cargo.
pub fn run(args: &[String], cwd: &Path) -> i32 {
    let code = run_command(args, cwd);
    match restore_target(ansi(std::io::stdout().is_terminal()), ansi(std::io::stderr().is_terminal())) {
        Some(Stream::Stdout) => print!("{}", rmx_terminal::ansi::RESET),
        Some(Stream::Stderr) => eprint!("{}", rmx_terminal::ansi::RESET),
        None => {}
    }
    code
}

/// An output stream.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

/// Remix `canUseAnsi`: a terminal that is not `TERM=dumb` (colors may still be off).
fn can_use_ansi(term: Option<&str>, is_tty: bool) -> bool {
    term != Some("dumb") && is_tty
}

/// Where Remix's `restoreTerminalFormatting` writes the closing reset: stdout when it takes
/// ANSI, else stderr when it does, else nowhere.
fn restore_target(stdout_ansi: bool, stderr_ansi: bool) -> Option<Stream> {
    if stdout_ansi {
        Some(Stream::Stdout)
    } else if stderr_ansi {
        Some(Stream::Stderr)
    } else {
        None
    }
}

/// Remix's command header (`REMIX v1.2.3 - doctor`) after the blank line that opens a
/// command's output: the header only on a terminal, with the wordmark colored a letter at a
/// time like Remix's (R blue, M yellow, X red).
fn preamble(command: &str, styles: &Styles, is_tty: bool) -> String {
    if !is_tty {
        return "\n".into();
    }
    let wordmark = format!("{}{}{}", styles.blue_bright("R"), styles.yellow_bright("M"), styles.red_bright("X"));
    format!("\n{wordmark} v{} - {command}\n\n", version())
}

/// Whether a stream that `is_tty` takes ANSI under this process's `TERM`.
fn ansi(is_tty: bool) -> bool {
    can_use_ansi(std::env::var("TERM").ok().as_deref(), is_tty)
}

fn run_command(args: &[String], cwd: &Path) -> i32 {
    let (args, global) = global_options(args);
    let args = &args[..];
    let colors = |is_tty: bool| !global.no_color && rmx_terminal::should_use_colors(|name| std::env::var(name).ok(), is_tty);
    let help = |args: &[String]| help_for(args.first().map(String::as_str).filter(|c| *c != "help")).unwrap_or(HELP);
    // Help under an error goes to stderr, so it follows stderr's colors.
    let error_help = |text: &str| style_help(text, &Styles::new(colors(std::io::stderr().is_terminal())));
    match parse(args) {
        Ok(Invocation::Help(text)) => {
            print!("{}", style_help(text, &Styles::new(colors(std::io::stdout().is_terminal()))));
            0
        }
        Ok(Invocation::Print(text)) => {
            print!("{text}");
            0
        }
        Ok(Invocation::Exec(argv)) => match exec_command(&argv, cwd, &global).status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(e) => {
                eprint!("{}", CliError::internal(format!("Could not run {}: {e}", argv[0])).render(""));
                1
            }
        },
        Ok(Invocation::New(options)) => {
            let is_tty = std::io::stderr().is_terminal();
            let styles = Styles::new(colors(is_tty));
            eprint!("{}", preamble("new", &styles, is_tty));
            let mut status = status::Status::new(Arc::new(Mutex::new(std::io::stderr())), styles, ansi(is_tty));
            let result = new_project(&options, cwd, &mut |line| status.line(line));
            drop(status);
            match result {
                Ok(app) => {
                    eprintln!();
                    println!("Created {} at {}", app.display_name, display_path(&app.target_dir, cwd));
                    0
                }
                Err(e) => {
                    eprintln!();
                    eprint!("{}", e.render(&error_help(NEW_HELP)));
                    1
                }
            }
        }
        Ok(Invocation::Dev(app_args)) => match dev::run_dev(cwd, &app_args) {
            Ok(()) => 0,
            Err(e) => {
                eprint!("{}", e.render(&error_help(DEV_HELP)));
                1
            }
        },
        Ok(Invocation::Doctor { json, strict, fix }) => {
            let is_tty = std::io::stdout().is_terminal();
            let styles = Styles::new(colors(is_tty));
            let result = if json {
                doctor::run_doctor_with(cwd, &doctor::SystemToolchain, fix)
            } else {
                print!("{}", preamble("doctor", &styles, is_tty));
                let mut status = status::Status::new(Arc::new(Mutex::new(std::io::stdout())), styles, ansi(is_tty));
                let mut running = None;
                let result = doctor::run_doctor_reporting(cwd, &doctor::SystemToolchain, fix, &mut |event| report_doctor(&mut status, &mut running, event, &styles));
                match &result {
                    Ok(report) => status.text(&report.summary()),
                    // The suite whose fix failed ends like a failed step.
                    Err(_) => running.into_iter().for_each(|name| status.line(&format!("\u{2717} {name}"))),
                }
                result
            };
            match result {
                Ok(report) => {
                    if json {
                        print!("{}", report.to_json());
                    }
                    doctor_exit_code(strict, fix, report.warnings())
                }
                Err(message) => {
                    eprint!("{}", CliError::internal(message).render(&error_help(DOCTOR_HELP)));
                    1
                }
            }
        }
        Err(e) => {
            eprint!("{}", e.render(&error_help(help(args))));
            1
        }
    }
}

/// Remix `formatHelpText` colors: the section headings bold light blue, and in usage lines,
/// examples and the labels of option and command rows, flags (`--json`, `-h`), bracketed
/// names (`[options]`) and angle-bracketed values (`<path>`) light yellow.
fn style_help(text: &str, styles: &Styles) -> String {
    if !styles.enabled() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() * 2);
    let mut section = "";
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        match body.strip_suffix(':') {
            Some(title @ ("Usage" | "Commands" | "Options" | "Examples")) => {
                section = title;
                out.push_str(&styles.format(title, &[rmx_terminal::Style::Bold, rmx_terminal::Style::BlueBright]));
                out.push(':');
            }
            _ => match (section, body.strip_prefix("  ")) {
                ("Usage" | "Examples", Some(rest)) => {
                    out.push_str("  ");
                    out.push_str(&highlight_syntax(rest, styles));
                }
                ("Commands" | "Options", Some(rest)) => {
                    // The label, padded to the description column.
                    let label_end = rest.find("  ").map_or(rest.len(), |gap| gap + rest[gap..].len() - rest[gap..].trim_start().len());
                    out.push_str("  ");
                    out.push_str(&highlight_syntax(&rest[..label_end], styles));
                    out.push_str(&rest[label_end..]);
                }
                _ => out.push_str(body),
            },
        }
        out.push_str(&line[body.len()..]);
    }
    out
}

/// `text` with Remix's help syntax tokens in light yellow.
fn highlight_syntax(text: &str, styles: &Styles) -> String {
    let bytes = text.as_bytes();
    let (mut out, mut plain, mut i) = (String::new(), 0, 0);
    while i < bytes.len() {
        match syntax_token(bytes, i) {
            Some(end) => {
                out.push_str(&text[plain..i]);
                out.push_str(&styles.yellow_bright(&text[i..end]));
                (plain, i) = (end, end);
            }
            None => i += 1,
        }
    }
    out.push_str(&text[plain..]);
    out
}

/// The end of the token at `i`, as Remix's pattern
/// `(?<![^\s[,|])(--[A-Za-z0-9-]+|-[A-Za-z])(?=$|[\s,\]|])|\[[A-Za-z][A-Za-z0-9-]*\]|<[^>\n]+>`.
fn syntax_token(b: &[u8], i: usize) -> Option<usize> {
    let name_end = |mut j: usize| {
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-') {
            j += 1;
        }
        j
    };
    let ends_flag = |j: usize| j == b.len() || b[j].is_ascii_whitespace() || matches!(b[j], b',' | b']' | b'|');
    let starts_flag = i == 0 || b[i - 1].is_ascii_whitespace() || matches!(b[i - 1], b'[' | b',' | b'|');
    match b[i] {
        b'-' if starts_flag && b.get(i + 1) == Some(&b'-') => Some(name_end(i + 2)).filter(|&j| j > i + 2 && ends_flag(j)),
        b'-' if starts_flag && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) => Some(i + 2).filter(|&j| ends_flag(j)),
        b'[' if b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) => Some(name_end(i + 2)).filter(|&j| b.get(j) == Some(&b']')).map(|j| j + 1),
        b'<' => {
            let close = b[i + 1..].iter().position(|&c| c == b'>' || c == b'\n').map(|p| i + 1 + p)?;
            (close > i + 1 && b[close] == b'>').then_some(close + 1)
        }
        _ => None,
    }
}

/// Doctor progress on `status` as the suites run: the running line, then the result line and
/// the suite's findings. `running` is the suite being checked.
fn report_doctor(status: &mut status::Status, running: &mut Option<&'static str>, event: doctor::Progress, styles: &Styles) {
    match event {
        doctor::Progress::Start(name) => {
            *running = Some(name);
            status.line(&doctor::running_line(name));
        }
        doctor::Progress::Done(suite) => {
            *running = None;
            status.line(&suite.result_line());
            status.text(&suite.details(styles));
        }
    }
}

/// Remix `run.ts`: warnings fail the command under `--strict` and under `--fix` (what
/// the fixes could not repair).
fn doctor_exit_code(strict: bool, fix: bool, warnings: usize) -> i32 {
    i32::from((strict || fix) && warnings > 0)
}

/// The command `rmx` hands over to (`cargo run`, `cargo test`); `--no-color` reaches it
/// as `NO_COLOR` (read by the app and `rmx_terminal`) and `CARGO_TERM_COLOR=never`.
pub fn exec_command(argv: &[String], cwd: &Path, global: &GlobalOptions) -> std::process::Command {
    let mut command = std::process::Command::new(&argv[0]);
    command.args(&argv[1..]).current_dir(cwd);
    if global.no_color {
        command.env("NO_COLOR", "1").env("CARGO_TERM_COLOR", "never");
    }
    command
}

/// The result of [`new_project`].
#[derive(Debug, PartialEq, Eq)]
pub struct NewApp {
    pub display_name: String,
    pub package_name: String,
    pub target_dir: PathBuf,
}

/// The starter app's files, from the repository's `template/`.
const TEMPLATE: &[(&str, &str)] = &[
    ("src/lib.rs", include_str!("../../../template/src/lib.rs")),
    ("src/main.rs", include_str!("../../../template/src/main.rs")),
    ("src/prompt_button.rs", include_str!("../../../template/src/prompt_button.rs")),
    ("src/routes.rs", include_str!("../../../template/src/routes.rs")),
    ("src/server/mod.rs", include_str!("../../../template/src/server/mod.rs")),
    ("src/server/home_page.rs", include_str!("../../../template/src/server/home_page.rs")),
    ("src/server/tests.rs", include_str!("../../../template/src/server/tests.rs")),
    ("app/actions/public/entry.js", include_str!("../../../template/app/actions/public/entry.js")),
    ("public/favicon.svg", include_str!("../../../template/public/favicon.svg")),
];

/// The default document title in the template, replaced by the app's display name.
const TEMPLATE_TITLE: &str = "unwrap_or(\"rmx App\")";

/// Create an app in `options.target_dir` (relative to `cwd`): Remix's `bootstrapProject`.
/// `progress` receives `• Step...` when a step starts and `✓ Step` or `✗ Step` when it ends.
/// The app depends on `rmx` by path (this crate's directory) unless `RMX_VERSION` names a
/// version.
pub fn new_project(options: &NewOptions, cwd: &Path, progress: &mut dyn FnMut(&str)) -> Result<NewApp, CliError> {
    let target_dir = normalize(&cwd.join(&options.target_dir));
    let raw_name = match &options.app_name {
        Some(name) => name.clone(),
        None => target_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    if raw_name.is_empty() {
        return Err(CliError::usage(
            "RMX_APP_NAME_UNAVAILABLE",
            "Could not determine an app name",
            "Pass --app-name or choose a target directory name that can become an app name.",
            "Could not determine an app name from the target directory.".into(),
        ));
    }
    let display_name = options.app_name.clone().unwrap_or_else(|| humanize_name(&raw_name));
    let package_name = to_package_name(&raw_name)?;
    let dependency = rmx_dependency();

    step(progress, "Prepare target directory", || prepare_target_directory(&target_dir, options.force))?;
    step(progress, "Generate scaffold files", || {
        for (file, source) in TEMPLATE {
            write_file(&target_dir.join(file), &render_template(file, source, &package_name, &display_name))?;
        }
        write_file(&target_dir.join("README.md"), &readme(&display_name, &package_name))?;
        write_file(&target_dir.join("AGENTS.md"), &agents_guide(&display_name))?;
        write_file(&target_dir.join(".gitignore"), "/target\n/app/actions/public/pkg\n")
    })?;
    step(progress, "Finalize Cargo.toml", || write_file(&target_dir.join("Cargo.toml"), &cargo_toml(&package_name, &dependency)))?;
    Ok(NewApp { display_name, package_name, target_dir })
}

/// A progress line in Remix's reporter colors: `• Step...` in gray, and `✓` green or `✗`
/// red before a gray label. Other lines are left alone.
pub(crate) fn style_progress(line: &str, styles: &Styles) -> String {
    let mut chars = line.chars();
    match (chars.next(), chars.as_str().strip_prefix(' ')) {
        (Some('\u{2022}'), Some(_)) => styles.gray(line),
        (Some('\u{2713}'), Some(label)) => format!("{} {}", styles.green_bright("\u{2713}"), styles.gray(label)),
        (Some('\u{2717}'), Some(label)) => format!("{} {}", styles.red_bright("\u{2717}"), styles.gray(label)),
        _ => line.to_string(),
    }
}

fn step(progress: &mut dyn FnMut(&str), label: &str, run: impl FnOnce() -> Result<(), CliError>) -> Result<(), CliError> {
    progress(&format!("\u{2022} {label}..."));
    let result = run();
    progress(&format!("{} {label}", if result.is_ok() { '\u{2713}' } else { '\u{2717}' }));
    result
}

fn prepare_target_directory(dir: &Path, force: bool) -> Result<(), CliError> {
    match std::fs::metadata(dir) {
        Ok(meta) if !meta.is_dir() => {
            return Err(CliError::usage(
                "RMX_TARGET_PATH_NOT_DIRECTORY",
                "Target path is not a directory",
                "Choose a directory path for the new app target.",
                format!("Target path is not a directory: {}", dir.display()),
            ))
        }
        Ok(_) => {
            let non_empty = std::fs::read_dir(dir).map_err(|e| io_error(dir, e))?.next().is_some();
            if non_empty && !force {
                return Err(CliError::usage(
                    "RMX_TARGET_DIRECTORY_NOT_EMPTY",
                    "Target directory is not empty",
                    "Re-run with --force to continue.",
                    format!("Target directory is not empty: {}. Re-run with --force to continue.", dir.display()),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(dir, e)),
    }
    std::fs::create_dir_all(dir).map_err(|e| io_error(dir, e))
}

/// The `rmx = ...` dependency line for new apps and `doctor --fix`: the version in
/// `RMX_VERSION`, or this checkout of rmx by path.
pub(crate) fn rmx_dependency() -> String {
    match std::env::var("RMX_VERSION").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(version) => format!("rmx = {}", toml_string(&version)),
        None => format!("rmx = {{ path = {} }}", toml_string(&env!("CARGO_MANIFEST_DIR").replace('\\', "/"))),
    }
}

fn write_file(path: &Path, content: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    }
    std::fs::write(path, content).map_err(|e| io_error(path, e))
}

fn io_error(path: &Path, e: std::io::Error) -> CliError {
    CliError::internal(format!("{}: {e}", path.display()))
}

/// A template file for the new app: the template's crate names become the app's, its title
/// becomes the display name, and repository-only lines are dropped.
fn render_template(file: &str, source: &str, package_name: &str, display_name: &str) -> String {
    let ident = package_name.replace('-', "_");
    let mut out = String::with_capacity(source.len());
    let mut removing = false;
    for line in source.split_inclusive('\n') {
        match line.trim() {
            "// rmx-template:remove-start" => removing = true,
            "// rmx-template:remove-end" => removing = false,
            _ if removing => {}
            _ => out.push_str(line),
        }
    }
    let mut out = out.replace("template_demo", &ident).replace("template-demo", package_name);
    if file == "src/main.rs" {
        // The repository runs the template from the workspace root; a new app runs from its own.
        out = out.replace("\"template\"", "\".\"").replace("(template)", "(.)");
    }
    if file == "src/server/home_page.rs" {
        // A Rust string literal, so quotes and backslashes in the name stay text.
        out = out.replace(TEMPLATE_TITLE, &format!("unwrap_or({display_name:?})"));
    }
    out
}

fn cargo_toml(package_name: &str, dependency: &str) -> String {
    format!(
        r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
{dependency}
serde = {{ version = "1", features = ["derive"] }}

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.129"
wasm-bindgen-futures = "0.4"
web-sys = {{ version = "0.3", features = ["Clipboard", "Navigator", "Window"] }}

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
http = "1"
tokio = {{ version = "1", features = ["rt-multi-thread"] }}

# Size-optimized browser build.
[profile.wasm-release]
inherits = "release"
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
"#
    )
}

fn readme(display_name: &str, package_name: &str) -> String {
    let ident = package_name.replace('-', "_");
    format!(
        "# {display_name}

A minimal rmx application starter with a home page.

## Starter Shape

- `src/routes.rs` defines the shared route contract used by the server and the browser for typed hrefs.
- `src/server/mod.rs` owns the asset server and wires the routes to their actions.
- `src/server/home_page.rs` renders the route-owned starter UI.
- `src/prompt_button.rs` is the interactive client entry; `src/lib.rs` starts it in the browser.
- `app/actions/public/` holds the browser entry script and the compiled client (`pkg/`).
- Root `public/` contains static files served unchanged.

## Commands

Build the client once (and after client changes), then run the server:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
cargo build --lib --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir app/actions/public/pkg target/wasm32-unknown-unknown/wasm-release/{ident}.wasm
cargo run
```

```sh
cargo test
cargo run -- routes
cargo run -- assets
NODE_ENV=production cargo run --release
```
"
    )
}

fn agents_guide(display_name: &str) -> String {
    format!(
        "# {display_name} Agent Guide

This app was scaffolded with `rmx new`. Use these conventions when continuing to build it out.

## Starter Layout

- `src/routes.rs` defines the shared route contract; add routes to the `Routes` struct and the compiler lists the actions the controller must provide.
- Put top-level route actions in `src/server/mod.rs`; `src/server/tests.rs` is the router smoke test.
- `src/server/home_page.rs` renders the starter UI; client entries live next to it as components with `rmx::client_entry!`.
- `app/actions/public/` contains the browser entry and the compiled client.
- Root `public/` contains static files served unchanged from the app root.

This starter intentionally begins small; add modules such as `src/data.rs` or `src/ui/` only when you need them.
"
    )
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `my_cool app` becomes `My Cool App` (`humanizeName`).
fn humanize_name(value: &str) -> String {
    let parts: Vec<String> = value
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        })
        .collect();
    if parts.is_empty() {
        "rmx App".into()
    } else {
        parts.join(" ")
    }
}

/// A Cargo package name from an app name (`toPackageName`): lowercase ASCII letters and
/// digits joined by `-`, not starting with a digit and not a reserved name.
fn to_package_name(value: &str) -> Result<String, CliError> {
    let mut name = String::new();
    for c in value.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            name.push(c);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_matches('-').to_string();
    const RESERVED: &[&str] = &[
        "test", "std", "core", "alloc", "proc-macro", "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
        "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
    ];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || RESERVED.contains(&name.as_str()) {
        return Err(CliError::usage(
            "RMX_INVALID_PACKAGE_NAME",
            "Could not derive a valid package name",
            "Choose an app name that can be normalized into a valid Cargo package name.",
            format!("Could not derive a valid package name from {value:?}."),
        ));
    }
    Ok(name)
}

/// `a/./b/../c` becomes `a/c` without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to `cwd` when inside it, else as is.
fn display_path(path: &Path, cwd: &Path) -> String {
    match path.strip_prefix(cwd) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests;
