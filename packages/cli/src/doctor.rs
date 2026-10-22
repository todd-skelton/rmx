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
//! compile time (`<Name>Actions` must name every route), so it has no counterpart.
//! `Cargo.toml` is read line by line, which covers the usual layouts.
//!
//! `--fix` applies the low-risk fixes, like Remix: it adds the `rmx` dependency, creates
//! `src/routes.rs` (or replaces one holding only comments) and declares it in `src/lib.rs`,
//! and adds `"cdylib"` to the crate types. Fixes are planned per suite and the suite is
//! checked again after them; a fix whose path would leave the project root (including
//! through a symlink) is refused, and a file to create that already exists is left alone.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rmx_terminal::Styles;

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
    /// Fixes `--fix` applied for this suite.
    pub applied_fixes: Vec<AppliedFix>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixKind {
    CreateFile,
    CreateDirectory,
    UpdateFile,
}

impl FixKind {
    fn as_str(self) -> &'static str {
        match self {
            FixKind::CreateFile => "create-file",
            FixKind::CreateDirectory => "create-directory",
            FixKind::UpdateFile => "update-file",
        }
    }
}

/// A change `--fix` will make (Remix's `DoctorFixPlan`): `path` is relative to the project
/// root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixPlan {
    pub code: &'static str,
    pub suite: &'static str,
    pub kind: FixKind,
    pub path: String,
    pub contents: String,
}

/// A fix that was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedFix {
    pub code: &'static str,
    pub suite: &'static str,
    pub kind: FixKind,
    pub path: String,
}

impl AppliedFix {
    fn describe(&self) -> String {
        match self.kind {
            FixKind::UpdateFile => format!("Updated {}", self.path),
            _ => format!("Created {}", self.path),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The directory holding the app's `Cargo.toml`, when found.
    pub project_root: Option<PathBuf>,
    pub suites: Vec<Suite>,
    /// Whether this was a `--fix` run (the JSON then lists applied fixes and what remains).
    pub fixed: bool,
}

impl Report {
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.suites.iter().flat_map(|s| s.findings.iter())
    }

    pub fn warnings(&self) -> usize {
        self.findings().filter(|f| f.severity == Severity::Warn).count()
    }

    pub fn applied_fixes(&self) -> impl Iterator<Item = &AppliedFix> {
        self.suites.iter().flat_map(|s| s.applied_fixes.iter())
    }

    /// The text report: each suite's result and findings, then a summary.
    pub fn render(&self) -> String {
        self.render_styled(&Styles::new(false))
    }

    /// [`Report::render`] with Remix's colors: gray progress, a green `✓` or red `✗`, and
    /// warnings in yellow.
    pub fn render_styled(&self, styles: &Styles) -> String {
        let mut out = String::from("\n");
        for suite in &self.suites {
            if !matches!(suite.status, SuiteStatus::Skipped(_)) {
                out.push_str(&format!("{}\n", super::style_progress(&running_line(suite.name), styles)));
            }
            out.push_str(&format!("{}\n", super::style_progress(&suite.result_line(), styles)));
            out.push_str(&suite.details(styles));
        }
        out.push_str(&self.summary());
        out
    }

    /// The closing lines: applied fixes, "no issues", and the warning and advice counts.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        let warnings = self.warnings();
        let advice = self.findings().count() - warnings;
        let fixes = self.applied_fixes().count();
        if fixes > 0 {
            out.push_str(&format!("Applied {fixes} {}.\n", if fixes == 1 { "fix" } else { "fixes" }));
        }
        if warnings + advice == 0 {
            out.push_str("Doctor found no issues.\n");
        }
        out.push_str(&format!("Summary: {warnings} warnings, {advice} advice.\n"));
        out
    }
}

/// The progress line of a suite that starts (Remix's running label).
pub fn running_line(name: &str) -> String {
    format!("\u{2022} Checking {name}...")
}

impl Suite {
    /// The progress line that ends the suite: `✓ name`, `✗ name` or `• name (skipped: why)`.
    pub fn result_line(&self) -> String {
        match &self.status {
            SuiteStatus::Ok => format!("\u{2713} {}", self.name),
            SuiteStatus::Issues => format!("\u{2717} {}", self.name),
            SuiteStatus::Skipped(reason) => format!("\u{2022} {} (skipped: {reason})", self.name),
        }
    }

    /// The findings and applied fixes under the result line, and the blank line after them.
    pub fn details(&self, styles: &Styles) -> String {
        let mut out = String::new();
        for finding in &self.findings {
            let label = format!("[{}] {}", finding.severity.as_str().to_uppercase(), finding.message);
            let label = if finding.severity == Severity::Warn { styles.yellow_bright(&label) } else { label };
            out.push_str(&format!("  \u{2022} {label}\n"));
        }
        if !self.applied_fixes.is_empty() {
            out.push_str("  Applied fixes:\n");
            for fix in &self.applied_fixes {
                out.push_str(&format!("    \u{2022} {}\n", fix.describe()));
            }
        }
        out.push('\n');
        out
    }
}

impl Report {
    /// The report as JSON (`--json`).
    pub fn to_json(&self) -> String {
        let applied = |f: &AppliedFix| format!(r#"{{"code":{},"kind":"{}","path":{},"suite":"{}"}}"#, json_string(f.code), f.kind.as_str(), json_string(&f.path), f.suite);
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
                let fixes = if s.applied_fixes.is_empty() { String::new() } else { format!(r#","appliedFixes":[{}]"#, s.applied_fixes.iter().map(applied).collect::<Vec<_>>().join(",")) };
                format!(r#"{{"name":"{}","status":"{status}"{reason},"findings":[{}]{fixes}}}"#, s.name, findings.join(","))
            })
            .collect();
        let findings: Vec<String> = self.suites.iter().flat_map(|s| s.findings.iter().map(|f| finding(f, s.name))).collect();
        let root = self.project_root.as_ref().map_or("null".into(), |p| json_string(&p.display().to_string()));
        let fixed = if self.fixed {
            let applied: Vec<String> = self.applied_fixes().map(applied).collect();
            format!(r#","appliedFixes":[{}],"remainingFindings":[{}]"#, applied.join(","), findings.join(","))
        } else {
            String::new()
        };
        format!(r#"{{"projectRoot":{root},"findings":[{}],"suites":[{}]{fixed}}}"#, findings.join(","), suites.join(",")) + "\n"
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
    run_doctor_with(cwd, toolchain, false).expect("no fixes to apply")
}

/// [`run_doctor`], applying each suite's fixes first when `fix` is set. Fails when a fix
/// cannot be applied (a path outside the project root, or an I/O error).
pub fn run_doctor_with(cwd: &Path, toolchain: &dyn Toolchain, fix: bool) -> Result<Report, String> {
    run_doctor_reporting(cwd, toolchain, fix, &mut |_| {})
}

/// What [`run_doctor_reporting`] tells its caller as it goes.
#[derive(Debug)]
pub enum Progress<'a> {
    /// A suite starts checking.
    Start(&'static str),
    /// A suite is done (or skipped).
    Done(&'a Suite),
}

/// [`run_doctor_with`], reporting each suite as it starts and ends (Remix's doctor progress).
pub fn run_doctor_reporting(cwd: &Path, toolchain: &dyn Toolchain, fix: bool, progress: &mut dyn FnMut(Progress)) -> Result<Report, String> {
    fn done(suite: Suite, suites: &mut Vec<Suite>, progress: &mut dyn FnMut(Progress)) {
        progress(Progress::Done(&suite));
        suites.push(suite);
    }
    let mut suites = Vec::new();
    let project_root = cwd.ancestors().find(|dir| dir.join("Cargo.toml").is_file() && read(&dir.join("Cargo.toml")).contains("[package]"));
    let manifest = |root: &Path| read(&root.join("Cargo.toml"));
    let env_check = || suite("environment", environment(project_root, &project_root.map(manifest).unwrap_or_default(), toolchain));
    progress(Progress::Start("environment"));
    let environment = match project_root {
        Some(root) if fix => fixed(root, env_check, environment_fixes)?,
        _ => env_check(),
    };
    done(environment, &mut suites, progress);
    let Some(root) = project_root else {
        for name in ["project", "client"] {
            done(skipped(name, "environment"), &mut suites, progress);
        }
        return Ok(Report { project_root: None, suites, fixed: fix });
    };
    type Check = fn(&Path, &str) -> Vec<Finding>;
    type Fixes = fn(&Path, &str) -> Vec<FixPlan>;
    for (name, check, fixes) in [("project", project as Check, project_fixes as Fixes), ("client", client, client_fixes)] {
        if let Some(blocker) = suites.iter().find(|s| s.status == SuiteStatus::Issues) {
            let blocker = blocker.name;
            done(skipped(name, blocker), &mut suites, progress);
            continue;
        }
        progress(Progress::Start(name));
        let run = || suite(name, check(root, &manifest(root)));
        let checked = if fix { fixed(root, run, fixes)? } else { run() };
        done(checked, &mut suites, progress);
    }
    Ok(Report { project_root: Some(root.to_path_buf()), suites, fixed: fix })
}

/// Apply a suite's fixes, then check it again.
fn fixed(root: &Path, check: impl Fn() -> Suite, fixes: fn(&Path, &str) -> Vec<FixPlan>) -> Result<Suite, String> {
    let plans = fixes(root, &read(&root.join("Cargo.toml")));
    if plans.is_empty() {
        return Ok(check());
    }
    let applied = apply_fix_plans(root, &plans)?;
    let mut suite = check();
    suite.applied_fixes = applied;
    Ok(suite)
}

fn skipped(name: &'static str, blocker: &str) -> Suite {
    Suite { name, status: SuiteStatus::Skipped(format!("Blocked by {blocker} warnings.")), findings: Vec::new(), applied_fixes: Vec::new() }
}

fn suite(name: &'static str, findings: Vec<Finding>) -> Suite {
    let status = if findings.iter().any(|f| f.severity == Severity::Warn) { SuiteStatus::Issues } else { SuiteStatus::Ok };
    Suite { name, status, findings, applied_fixes: Vec::new() }
}

// fixes ---------------------------------------------------------------------------------

/// Make `plans` (Remix's `applyDoctorFixPlans`): duplicates are made once, a file to create
/// that exists is skipped (and not reported), and nothing is written outside `root`.
pub fn apply_fix_plans(root: &Path, plans: &[FixPlan]) -> Result<Vec<AppliedFix>, String> {
    let mut applied = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for plan in plans {
        if !seen.insert((plan.kind, plan.path.clone())) {
            continue;
        }
        let path = contained_fix_path(root, &plan.path)?;
        let io = |e: std::io::Error| format!("{}: {e}", path.display());
        match plan.kind {
            FixKind::CreateDirectory => std::fs::create_dir_all(&path).map_err(io)?,
            kind => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                if kind == FixKind::UpdateFile {
                    std::fs::write(&path, &plan.contents).map_err(io)?;
                } else {
                    use std::io::Write;
                    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                        Ok(mut file) => file.write_all(plan.contents.as_bytes()).map_err(io)?,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(e) => return Err(io(e)),
                    }
                }
            }
        }
        applied.push(AppliedFix { code: plan.code, suite: plan.suite, kind: plan.kind, path: plan.path.clone() });
    }
    Ok(applied)
}

/// `relative` under `root`, refusing `..` escapes, absolute paths and existing ancestors
/// (or the file itself) whose real path leaves the root (Remix's `resolveDoctorFixPath`).
fn contained_fix_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let outside = || format!("Doctor fix path resolves outside the app root: {relative}");
    let mut parts = Vec::new();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_owned()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(outside());
                }
            }
            _ => return Err(outside()),
        }
    }
    let real_root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    // The real path of the deepest existing ancestor; what does not exist yet goes below it.
    let mut real = real_root.clone();
    let mut current = root.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        current.push(part);
        if std::fs::symlink_metadata(&current).is_err() {
            real.extend(&parts[i..]);
            return Ok(real);
        }
        real = current.canonicalize().map_err(|e| format!("{}: {e}", current.display()))?;
        if !real.starts_with(&real_root) {
            return Err(outside());
        }
    }
    Ok(real)
}

fn environment_fixes(_root: &Path, manifest: &str) -> Vec<FixPlan> {
    if section_lines(manifest, "dependencies").any(|line| dependency_name(line) == Some("rmx")) {
        return Vec::new();
    }
    let dependency = super::rmx_dependency();
    let updated = match manifest.lines().position(|line| line.trim() == "[dependencies]") {
        Some(i) => insert_line(manifest, i + 1, &dependency),
        None => format!("{}\n[dependencies]\n{dependency}\n", manifest.trim_end()),
    };
    vec![update("rmx-dependency-missing", "environment", "Cargo.toml", updated)]
}

const ROUTES_RS: &str = "//! The route map shared by the server and the browser.

use std::sync::LazyLock;

use rmx::router::routes::{any, Route, RouteMap};

#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { home: any(\"/\") });
";

fn project_fixes(root: &Path, _manifest: &str) -> Vec<FixPlan> {
    if !project(root, "").is_empty() {
        let mut plans = Vec::new();
        let routes = root.join("src/routes.rs");
        if !routes.exists() {
            plans.push(FixPlan { code: "routes-missing", suite: "project", kind: FixKind::CreateFile, path: "src/routes.rs".into(), contents: ROUTES_RS.into() });
        } else if only_comments(&read(&routes)) {
            plans.push(update("routes-missing", "project", "src/routes.rs", ROUTES_RS.into()));
        } else {
            return plans;
        }
        let lib = read(&root.join("src/lib.rs"));
        let declared = lib.lines().any(|l| matches!(l.trim(), "mod routes;" | "pub mod routes;" | "pub(crate) mod routes;"));
        if root.join("src/lib.rs").is_file() && !declared {
            plans.push(update("routes-missing", "project", "src/lib.rs", format!("{}\npub mod routes;\n", lib.trim_end())));
        }
        return plans;
    }
    Vec::new()
}

fn client_fixes(root: &Path, manifest: &str) -> Vec<FixPlan> {
    if !client(root, manifest).iter().any(|f| f.code == "cdylib-missing") {
        return Vec::new();
    }
    let lines: Vec<&str> = manifest.lines().collect();
    let updated = match lines.iter().position(|l| l.trim() == "[lib]") {
        Some(header) => {
            let crate_type = lines.iter().enumerate().skip(header + 1).take_while(|(_, l)| !l.trim_start().starts_with('[')).find(|(_, l)| l.trim_start().starts_with("crate-type"));
            match crate_type {
                Some((i, line)) => {
                    let Some(open) = line.find('[') else { return Vec::new() };
                    let inner = line[open + 1..].trim_start();
                    let item = if inner.starts_with(']') { "\"cdylib\"" } else { "\"cdylib\", " };
                    let mut lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
                    lines[i] = format!("{}{item}{inner}", &line[..=open]);
                    lines.join("\n") + "\n"
                }
                None => insert_line(manifest, header + 1, "crate-type = [\"cdylib\", \"rlib\"]"),
            }
        }
        None => {
            let at = lines.iter().position(|l| l.trim() == "[dependencies]").unwrap_or(lines.len());
            insert_line(manifest, at, "[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n")
        }
    };
    vec![update("cdylib-missing", "client", "Cargo.toml", updated)]
}

fn update(code: &'static str, suite: &'static str, path: &str, contents: String) -> FixPlan {
    FixPlan { code, suite, kind: FixKind::UpdateFile, path: path.into(), contents }
}

/// `text` with `line` inserted before line `at` (0-based), ending with a newline.
fn insert_line(text: &str, at: usize, line: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    lines.insert(at.min(lines.len()), line);
    lines.join("\n") + "\n"
}

/// Whether Rust source holds nothing but whitespace and comments.
fn only_comments(source: &str) -> bool {
    let mut rest = source;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return true;
        }
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |i| &after[i..]);
        } else if let Some(after) = rest.strip_prefix("/*") {
            match after.find("*/") {
                Some(i) => rest = &after[i + 2..],
                None => return true,
            }
        } else {
            return false;
        }
    }
}

fn warn(code: &'static str, message: impl Into<String>) -> Finding {
    Finding { code, severity: Severity::Warn, message: message.into() }
}

fn advice(code: &'static str, message: impl Into<String>) -> Finding {
    Finding { code, severity: Severity::Advice, message: message.into() }
}

fn environment(root: Option<&Path>, manifest: &str, toolchain: &dyn Toolchain) -> Vec<Finding> {
    let mut findings = Vec::new();
    if root.is_none() {
        findings.push(warn("project-root-not-found", "Could not find Cargo.toml. Run this command inside an rmx project."));
        return findings;
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
    findings
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
