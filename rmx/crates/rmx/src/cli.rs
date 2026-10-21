//! The `rmx` command (Remix's `remix` CLI): `new`, `dev`, `doctor`, `completion`, `routes`, `assets`, `test`,
//! `version` and `help`. Install it with `cargo install --path crates/rmx`.
//!
//! `new` writes the starter app (`examples/template`, Remix's `template`) as a standalone
//! crate. `routes` and `assets` need the app's own route map and asset server, so they run
//! the app binary (`cargo run --quiet -- routes`), whose `main` handles them with
//! [`routes_command`](crate::router::routes::routes_command) and
//! [`assets_command`](crate::assets::assets_command). `test` runs `cargo test`.

use std::fmt;
use std::io::IsTerminal;
use std::path::{Component, Path, PathBuf};

use rmx_terminal::Styles;

pub mod completion;
pub mod dev;
pub mod doctor;

/// What a command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    /// Print to stdout and exit 0 (help, version).
    Print(String),
    /// Run a program in the current directory and exit with its status.
    Exec(Vec<String>),
    /// Create an app.
    New(NewOptions),
    /// Check the toolchain and the app (`--json`, `--strict`).
    Doctor { json: bool, strict: bool },
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
  rmx doctor [--json] [--strict]

Check the toolchain and rmx app conventions for the current project.

Options:
  --json       Print doctor findings as JSON
  --strict     Exit with status 1 when warning-level findings are present
  --no-strict  Do not exit with status 1 when warning-level findings are present

Examples:
  rmx doctor
  rmx doctor --json
  rmx doctor --strict
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
  rmx test [filter]

Run tests for the current project (`cargo test [filter]`).
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

/// Parse a command line (without the program name).
pub fn parse(args: &[String]) -> Result<Invocation, CliError> {
    let mut args: Vec<&str> = args.iter().map(String::as_str).skip_while(|a| *a == "--").collect();
    if args.is_empty() || matches!(args[0], "-h" | "--help") {
        return Ok(Invocation::Print(HELP.into()));
    }
    let command = args.remove(0);
    let wants_help = args.iter().any(|a| matches!(*a, "-h" | "--help"));
    match command {
        "help" => match args.as_slice() {
            [] => Ok(Invocation::Print(HELP.into())),
            [command] => Ok(Invocation::Print(help_for(Some(command))?.into())),
            [_, extra, ..] => Err(CliError::unexpected_extra_argument(extra)),
        },
        "-v" | "--version" | "version" => {
            if wants_help && command == "version" {
                return Ok(Invocation::Print(VERSION_HELP.into()));
            }
            match args.first() {
                Some(arg) if arg.starts_with('-') => Err(CliError::unknown_argument(arg)),
                Some(arg) => Err(CliError::unexpected_extra_argument(arg)),
                None => Ok(Invocation::Print(format!("{}\n", version()))),
            }
        }
        "new" => {
            if args.is_empty() || wants_help {
                return Ok(Invocation::Print(NEW_HELP.into()));
            }
            parse_new(&args).map(Invocation::New)
        }
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
            [] => Ok(Invocation::Print(COMPLETION_HELP.into())),
            _ if wants_help => Ok(Invocation::Print(COMPLETION_HELP.into())),
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
                return Ok(Invocation::Print(DEV_HELP.into()));
            }
            if let Some(arg) = before.first() {
                return Err(if arg.starts_with('-') { CliError::unknown_argument(arg) } else { CliError::unexpected_extra_argument(arg) });
            }
            Ok(Invocation::Dev(after.iter().map(|a| a.to_string()).collect()))
        }
        "doctor" => {
            if wants_help {
                return Ok(Invocation::Print(DOCTOR_HELP.into()));
            }
            let (mut json, mut strict) = (false, false);
            for arg in &args {
                match *arg {
                    "--json" => json = true,
                    "--strict" => strict = true,
                    "--no-strict" => strict = false,
                    other if other.starts_with('-') => return Err(CliError::unknown_argument(other)),
                    other => return Err(CliError::unexpected_extra_argument(other)),
                }
            }
            Ok(Invocation::Doctor { json, strict })
        }
        "test" => {
            if wants_help {
                return Ok(Invocation::Print(TEST_HELP.into()));
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
    let help = |args: &[String]| help_for(args.first().map(String::as_str).filter(|c| *c != "help")).unwrap_or(HELP);
    match parse(args) {
        Ok(Invocation::Print(text)) => {
            print!("{text}");
            0
        }
        Ok(Invocation::Exec(argv)) => match std::process::Command::new(&argv[0]).args(&argv[1..]).current_dir(cwd).status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(e) => {
                eprint!("{}", CliError::internal(format!("Could not run {}: {e}", argv[0])).render(""));
                1
            }
        },
        Ok(Invocation::New(options)) => {
            let styles = Styles::new(rmx_terminal::should_use_colors(|name| std::env::var(name).ok(), std::io::stderr().is_terminal()));
            let mut progress = |line: &str| eprintln!("{}", style_progress(line, &styles));
            match new_project(&options, cwd, &mut progress) {
                Ok(app) => {
                    eprintln!();
                    println!("Created {} at {}", app.display_name, display_path(&app.target_dir, cwd));
                    0
                }
                Err(e) => {
                    eprintln!();
                    eprint!("{}", e.render(NEW_HELP));
                    1
                }
            }
        }
        Ok(Invocation::Dev(app_args)) => match dev::run_dev(cwd, &app_args) {
            Ok(()) => 0,
            Err(e) => {
                eprint!("{}", e.render(DEV_HELP));
                1
            }
        },
        Ok(Invocation::Doctor { json, strict }) => {
            let report = doctor::run_doctor(cwd, &doctor::SystemToolchain);
            print!("{}", if json { report.to_json() } else { report.render_styled(&Styles::detect()) });
            i32::from(strict && report.warnings() > 0)
        }
        Err(e) => {
            eprint!("{}", e.render(help(args)));
            1
        }
    }
}

/// The result of [`new_project`].
#[derive(Debug, PartialEq, Eq)]
pub struct NewApp {
    pub display_name: String,
    pub package_name: String,
    pub target_dir: PathBuf,
}

/// The starter app's files, from `examples/template`.
const TEMPLATE: &[(&str, &str)] = &[
    ("src/lib.rs", include_str!("../../../examples/template/src/lib.rs")),
    ("src/main.rs", include_str!("../../../examples/template/src/main.rs")),
    ("src/prompt_button.rs", include_str!("../../../examples/template/src/prompt_button.rs")),
    ("src/routes.rs", include_str!("../../../examples/template/src/routes.rs")),
    ("src/server/mod.rs", include_str!("../../../examples/template/src/server/mod.rs")),
    ("src/server/home_page.rs", include_str!("../../../examples/template/src/server/home_page.rs")),
    ("src/server/tests.rs", include_str!("../../../examples/template/src/server/tests.rs")),
    ("app/actions/public/entry.js", include_str!("../../../examples/template/app/actions/public/entry.js")),
    ("public/favicon.svg", include_str!("../../../examples/template/public/favicon.svg")),
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
    let dependency = match std::env::var("RMX_VERSION").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(version) => format!("rmx = {}", toml_string(&version)),
        None => format!("rmx = {{ path = {} }}", toml_string(&env!("CARGO_MANIFEST_DIR").replace('\\', "/"))),
    };

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
        out = out.replace("examples/template", ".");
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
