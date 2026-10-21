//! `rmx doctor` (Remix's `remix doctor`): check the toolchain and the app's conventions.
//!
//! Suites run in order and a suite with warnings skips the ones after it, as in Remix:
//!
//! 1. environment: a `Cargo.toml` with an `rmx` dependency, the `wasm32-unknown-unknown`
//!    target, and a `wasm-bindgen` CLI matching the version rmx pins.
//! 2. project: a route map (`#[derive(RouteMap)]`) under `src/`.
//! 3. client: for apps with a browser entry (`wasm_bindgen(start)`), a `cdylib` crate type
//!    and a built, current `<crate>_bg.wasm`.
//!
//! Remix's `actions` suite checks controller file conventions; rmx checks controllers at
//! compile time (`<Name>Actions` must name every route), so it has no counterpart. There are
//! no automatic fixes. `Cargo.toml` is read line by line, which covers the usual layouts.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The `wasm-bindgen` version rmx builds against (the `=0.2.129` pin).
pub const WASM_BINDGEN_VERSION: &str = "0.2.129";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Advice,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Warn => "warn",
            Severity::Advice => "advice",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuiteStatus {
    Ok,
    Issues,
    Skipped(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suite {
    pub name: &'static str,
    pub status: SuiteStatus,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The directory holding the app's `Cargo.toml`, when found.
    pub project_root: Option<PathBuf>,
    pub suites: Vec<Suite>,
}

impl Report {
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.suites.iter().flat_map(|s| s.findings.iter())
    }

    pub fn warnings(&self) -> usize {
        self.findings().filter(|f| f.severity == Severity::Warn).count()
    }

    /// The text report: each suite's result and findings, then a summary.
    pub fn render(&self) -> String {
        let mut out = String::from("\n");
        for suite in &self.suites {
            match &suite.status {
                SuiteStatus::Skipped(reason) => out.push_str(&format!("\u{2022} {} (skipped: {reason})\n", suite.name)),
                status => {
                    let mark = if *status == SuiteStatus::Ok { '\u{2713}' } else { '\u{2717}' };
                    out.push_str(&format!("\u{2022} Checking {}...\n{mark} {}\n", suite.name, suite.name));
                }
            }
            for finding in &suite.findings {
                out.push_str(&format!("  \u{2022} [{}] {}\n", finding.severity.as_str().to_uppercase(), finding.message));
            }
            out.push('\n');
        }
        let warnings = self.warnings();
        let advice = self.findings().count() - warnings;
        if warnings + advice == 0 {
            out.push_str("Doctor found no issues.\n");
        }
        out.push_str(&format!("Summary: {warnings} warnings, {advice} advice.\n"));
        out
    }

    /// The report as JSON (`--json`).
    pub fn to_json(&self) -> String {
        let finding = |f: &Finding, suite: &str| {
            format!(r#"{{"code":{},"severity":"{}","suite":"{suite}","message":{}}}"#, json_string(f.code), f.severity.as_str(), json_string(&f.message))
        };
        let suites: Vec<String> = self
            .suites
            .iter()
            .map(|s| {
                let (status, reason) = match &s.status {
                    SuiteStatus::Ok => ("ok", String::new()),
                    SuiteStatus::Issues => ("issues", String::new()),
                    SuiteStatus::Skipped(reason) => ("skipped", format!(r#","reason":{}"#, json_string(reason))),
                };
                let findings: Vec<String> = s.findings.iter().map(|f| finding(f, s.name)).collect();
                format!(r#"{{"name":"{}","status":"{status}"{reason},"findings":[{}]}}"#, s.name, findings.join(","))
            })
            .collect();
        let findings: Vec<String> = self.suites.iter().flat_map(|s| s.findings.iter().map(|f| finding(f, s.name))).collect();
        let root = self.project_root.as_ref().map_or("null".into(), |p| json_string(&p.display().to_string()));
        format!(r#"{{"projectRoot":{root},"findings":[{}],"suites":[{}]}}"#, findings.join(","), suites.join(",")) + "\n"
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Questions about the machine, answered by running tools (`SystemToolchain`) or by tests.
pub trait Toolchain {
    /// `rustup target list --installed`; `None` when rustup is unavailable.
    fn installed_targets(&self) -> Option<Vec<String>>;
    /// The version `wasm-bindgen --version` reports; `None` when it is not on `PATH`.
    fn wasm_bindgen_version(&self) -> Option<String>;
}

/// [`Toolchain`] backed by `rustup` and `wasm-bindgen` on `PATH`.
pub struct SystemToolchain;

impl Toolchain for SystemToolchain {
    fn installed_targets(&self) -> Option<Vec<String>> {
        let out = std::process::Command::new("rustup").args(["target", "list", "--installed"]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).collect())
    }

    fn wasm_bindgen_version(&self) -> Option<String> {
        let out = std::process::Command::new("wasm-bindgen").arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "wasm-bindgen 0.2.129"
        text.split_whitespace().nth(1).map(str::to_string)
    }
}

/// Run every suite for the app containing `cwd`.
pub fn run_doctor(cwd: &Path, toolchain: &dyn Toolchain) -> Report {
    let project_root = cwd.ancestors().find(|dir| dir.join("Cargo.toml").is_file() && read(&dir.join("Cargo.toml")).contains("[package]"));
    let manifest = project_root.map(|root| read(&root.join("Cargo.toml"))).unwrap_or_default();
    let mut suites = vec![environment(project_root, &manifest, toolchain)];
    let blocked = |suites: &[Suite]| suites.iter().find(|s| s.status == SuiteStatus::Issues).map(|s| format!("Blocked by {} warnings.", s.name));
    for (name, check) in [("project", project as fn(&Path, &str) -> Vec<Finding>), ("client", client)] {
        let suite = match blocked(&suites) {
            Some(reason) => Suite { name, status: SuiteStatus::Skipped(reason), findings: Vec::new() },
            None => suite(name, check(project_root.expect("environment passed"), &manifest)),
        };
        suites.push(suite);
    }
    Report { project_root: project_root.map(Path::to_path_buf), suites }
}

fn suite(name: &'static str, findings: Vec<Finding>) -> Suite {
    let status = if findings.iter().any(|f| f.severity == Severity::Warn) { SuiteStatus::Issues } else { SuiteStatus::Ok };
    Suite { name, status, findings }
}

fn warn(code: &'static str, message: impl Into<String>) -> Finding {
    Finding { code, severity: Severity::Warn, message: message.into() }
}

fn advice(code: &'static str, message: impl Into<String>) -> Finding {
    Finding { code, severity: Severity::Advice, message: message.into() }
}

fn environment(root: Option<&Path>, manifest: &str, toolchain: &dyn Toolchain) -> Suite {
    let mut findings = Vec::new();
    if root.is_none() {
        findings.push(warn("project-root-not-found", "Could not find Cargo.toml. Run this command inside an rmx project."));
        return suite("environment", findings);
    }
    if !section_lines(manifest, "dependencies").any(|line| dependency_name(line) == Some("rmx")) {
        findings.push(warn("rmx-dependency-missing", "Cargo.toml does not declare an rmx dependency."));
    }
    match toolchain.installed_targets() {
        None => findings.push(advice("rustup-unavailable", "Could not run rustup to check for the wasm32-unknown-unknown target.")),
        Some(targets) if !targets.iter().any(|t| t == "wasm32-unknown-unknown") => findings.push(warn(
            "wasm-target-missing",
            "The wasm32-unknown-unknown target is not installed. Run `rustup target add wasm32-unknown-unknown`.",
        )),
        Some(_) => {}
    }
    match toolchain.wasm_bindgen_version() {
        None => findings.push(advice(
            "wasm-bindgen-missing",
            format!("wasm-bindgen is not on PATH. Run `cargo install wasm-bindgen-cli --version {WASM_BINDGEN_VERSION}` to build the client."),
        )),
        Some(version) if version != WASM_BINDGEN_VERSION => findings.push(warn(
            "wasm-bindgen-version-mismatch",
            format!(
                "wasm-bindgen {version} is on PATH, but rmx uses {WASM_BINDGEN_VERSION}. Run `cargo install wasm-bindgen-cli --version {WASM_BINDGEN_VERSION}`."
            ),
        )),
        Some(_) => {}
    }
    suite("environment", findings)
}

fn project(root: &Path, _manifest: &str) -> Vec<Finding> {
    let sources = rust_sources(&root.join("src"));
    if !sources.iter().any(|(_, source)| source.contains("derive(RouteMap)")) {
        return vec![warn("routes-missing", "Project has no route map: add a struct with #[derive(RouteMap)] (src/routes.rs).")];
    }
    Vec::new()
}

fn client(root: &Path, manifest: &str) -> Vec<Finding> {
    let sources = rust_sources(&root.join("src"));
    if !sources.iter().any(|(_, source)| source.contains("wasm_bindgen(start)")) {
        return Vec::new();
    }
    let mut findings = Vec::new();
    let crate_types = section_lines(manifest, "lib").find(|line| line.trim_start().starts_with("crate-type"));
    if !crate_types.is_some_and(|line| line.contains("\"cdylib\"")) {
        findings.push(warn("cdylib-missing", "Cargo.toml [lib] crate-type does not include \"cdylib\", so the client cannot build to wasm."));
    }
    let name = package_name(manifest).replace('-', "_");
    let wasm_name = format!("{name}_bg.wasm");
    match find_file(root, &wasm_name) {
        None => findings.push(advice("client-not-built", format!("The client has not been built: no {wasm_name} outside target/ (see the README)."))),
        Some(wasm) => {
            let built = modified(&wasm);
            let newest_source = sources.iter().filter_map(|(path, _)| modified(path)).max();
            if let (Some(built), Some(newest)) = (built, newest_source) {
                if newest > built {
                    let rel = wasm.strip_prefix(root).unwrap_or(&wasm).components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                    findings.push(advice("client-stale", format!("The built client ({rel}) is older than src/; rebuild it.")));
                }
            }
        }
    }
    findings
}

pub(crate) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The lines of a `[section]` of a TOML file (up to the next header).
pub(crate) fn section_lines<'a>(toml: &'a str, section: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let header = format!("[{section}]");
    toml.lines().skip_while(move |line| line.trim() != header).skip(1).take_while(|line| !line.trim_start().starts_with('['))
}

/// The `[package]` name in a manifest.
pub(crate) fn package_name(manifest: &str) -> String {
    section_lines(manifest, "package")
        .find_map(|line| {
            let rest = line.trim().strip_prefix("name")?.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

/// `rmx = ...`, `rmx.workspace = true` and `rmx = { ... }` all name `rmx`.
fn dependency_name(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    let key = line.split(['=', '.']).next()?.trim();
    (!key.is_empty()).then_some(key.trim_matches('"'))
}

pub(crate) fn rust_sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            let source = read(&path);
            out.push((path, source));
        }
    }
    out
}

/// The first file called `name` below `dir`, skipping `target/` and hidden directories.
pub(crate) fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if path.is_dir() {
            if file_name != "target" && !file_name.starts_with('.') {
                dirs.push(path);
            }
        } else if file_name == name {
            return Some(path);
        }
    }
    dirs.into_iter().find_map(|d| find_file(&d, name))
}
