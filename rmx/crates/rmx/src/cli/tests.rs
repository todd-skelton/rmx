//! Ports of the CLI's cli.test.ts cases for `new`, `help` and `version`.

use super::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("rmx-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn new_app(cwd: &Path, list: &[&str]) -> (Result<NewApp, CliError>, Vec<String>) {
    let Invocation::New(options) = parse(&args(list)).unwrap() else { panic!("not a new command") };
    let mut lines = Vec::new();
    let result = new_project(&options, cwd, &mut |line| lines.push(line.to_string()));
    (result, lines)
}

#[test]
fn prints_help_and_the_version() {
    assert_eq!(parse(&[]), Ok(Invocation::Help(HELP)));
    assert_eq!(parse(&args(&["--help"])), Ok(Invocation::Help(HELP)));
    assert!(HELP.contains("new <name>        Create a new rmx project"));
    assert_eq!(parse(&args(&["help", "new"])), Ok(Invocation::Help(NEW_HELP)));
    assert_eq!(parse(&args(&["new"])), Ok(Invocation::Help(NEW_HELP)), "new without arguments prints its help");
    assert_eq!(parse(&args(&["new", "--help"])), Ok(Invocation::Help(NEW_HELP)));
    assert!(NEW_HELP.contains("Create a new rmx project in the target directory."));
    let version = format!("{}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(parse(&args(&["version"])), Ok(Invocation::Print(version.clone())));
    assert_eq!(parse(&args(&["-v"])), Ok(Invocation::Print(version.clone())));
    assert_eq!(parse(&args(&["--", "--version"])), Ok(Invocation::Print(version)));
}

#[test]
fn accepts_the_global_no_color_flag_before_a_separator() {
    let version = Ok(Invocation::Print(format!("{}\n", env!("CARGO_PKG_VERSION"))));
    assert_eq!(parse(&args(&["--no-color", "version"])), version);
    assert_eq!(parse(&args(&["--no-color", "--", "--version"])), version, "a separator after global options");
    assert_eq!(parse(&args(&["--", "--help"])), Ok(Invocation::Help(HELP)), "a leading separator");
    assert_eq!(parse(&args(&["version", "--no-color"])), version, "anywhere before a separator");
    assert!(HELP.contains("  --no-color     Disable ANSI color output\n"));

    let (rest, global) = global_options(&args(&["--no-color", "doctor", "--json"]));
    assert_eq!((rest, global.no_color), (args(&["doctor", "--json"]), true));
    // After `--` it belongs to the app: `rmx dev -- --no-color` passes it on.
    let (rest, global) = global_options(&args(&["dev", "--", "--no-color"]));
    assert_eq!((rest, global.no_color), (args(&["dev", "--", "--no-color"]), false));
    assert_eq!(parse(&args(&["dev", "--", "--no-color"])), Ok(Invocation::Dev(args(&["--no-color"]))));

    let envs = |global: &GlobalOptions| -> Vec<(String, String)> {
        let command = exec_command(&args(&["cargo", "test"]), Path::new("."), global);
        command.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()).unwrap_or_default())).collect()
    };
    assert_eq!(envs(&GlobalOptions { no_color: true }), vec![("CARGO_TERM_COLOR".to_string(), "never".to_string()), ("NO_COLOR".to_string(), "1".to_string())]);
    assert!(envs(&GlobalOptions::default()).is_empty());
}

#[test]
fn reports_unknown_commands_and_arguments() {
    let err = parse(&args(&["deploy"])).unwrap_err();
    assert_eq!((err.code, err.message.as_str()), ("RMX_UNKNOWN_COMMAND", "Unknown command: deploy"));
    let rendered = err.render(HELP);
    assert!(rendered.starts_with("Error [RMX_UNKNOWN_COMMAND] Unknown command\nUnknown command: deploy\n\nTry:\n  Run `rmx help` to see available commands.\n\nUsage:"), "{rendered}");
    let err = parse(&args(&["new", "my-app", "--remix-version", "1.2.3"])).unwrap_err();
    assert_eq!(err.message, "Unknown argument: --remix-version");
    assert_eq!(parse(&args(&["new", "a", "b"])).unwrap_err().message, "Unexpected extra argument: b");
    assert_eq!(parse(&args(&["new", "a", "--app-name"])).unwrap_err().message, "--app-name requires a value.");
    assert_eq!(parse(&args(&["new", "a", "--app-name="])).unwrap_err().message, "--app-name requires a value.");
    assert_eq!(parse(&args(&["new", "--force"])).unwrap_err().message, "A target directory is required.");
    assert_eq!(parse(&args(&["help", "nope"])).unwrap_err().message, "Unknown command: nope");
}

#[test]
fn routes_assets_and_test_run_the_app_through_cargo() {
    let exec = |list: &[&str]| match parse(&args(list)).unwrap() {
        Invocation::Exec(argv) => argv.join(" "),
        other => panic!("{other:?}"),
    };
    assert_eq!(exec(&["routes", "--table"]), "cargo run --quiet -- routes --table");
    assert_eq!(exec(&["assets", "inspect", "/assets/app/x.js"]), "cargo run --quiet -- assets inspect /assets/app/x.js");
    assert_eq!(exec(&["test", "frame"]), "cargo test frame");
    assert_eq!(exec(&["test", "--", "-h"]), "cargo test -- -h", "help flags after -- go to the test binaries");
    assert_eq!(exec(&["test", "-p", "app", "--", "--nocapture", "--help"]), "cargo test -p app -- --nocapture --help");
    assert_eq!(parse(&args(&["test", "frame", "-h", "--", "x"])), Ok(Invocation::Help(TEST_HELP)));
    assert_eq!(parse(&args(&["help", "test"])), parse(&args(&["test", "--help"])));
    // Help needs no project build (Remix prints it before looking for one).
    assert_eq!(parse(&args(&["routes", "--json", "-h"])), Ok(Invocation::Help(ROUTES_HELP)));
    assert_eq!(parse(&args(&["assets", "inspect", "--help"])), Ok(Invocation::Help(ASSETS_HELP)));
    assert_eq!(parse(&args(&["test", "--help"])), Ok(Invocation::Help(TEST_HELP)));
}

#[test]
fn creates_the_expected_scaffold() {
    let scratch = Scratch::new("scaffold");
    let (result, lines) = new_app(&scratch.0, &["new", "my-app", "--app-name", "My App"]);
    let app = result.unwrap();
    assert_eq!(app.display_name, "My App");
    assert_eq!(app.package_name, "my-app");
    assert_eq!(app.target_dir, scratch.0.join("my-app"));
    assert_eq!(
        lines,
        [
            "\u{2022} Prepare target directory...",
            "\u{2713} Prepare target directory",
            "\u{2022} Generate scaffold files...",
            "\u{2713} Generate scaffold files",
            "\u{2022} Finalize Cargo.toml...",
            "\u{2713} Finalize Cargo.toml",
        ]
    );
    let dir = &app.target_dir;
    let cargo = read(dir, "Cargo.toml");
    assert!(cargo.contains("name = \"my-app\""), "{cargo}");
    assert!(cargo.contains("crate-type = [\"cdylib\", \"rlib\"]"));
    assert!(cargo.contains("rmx = { path = ") && !cargo.contains('\\'), "{cargo}");
    assert!(cargo.contains("wasm-bindgen = \"=0.2.129\""));
    assert!(!cargo.contains("workspace"), "a standalone crate: {cargo}");

    let main = read(dir, "src/main.rs");
    assert!(main.contains("my_app::routes::ROUTES") && main.contains("my_app::server::router"), "{main}");
    assert!(main.contains("`cargo run -p my-app`"));
    assert!(!main.contains("template") && !main.contains("rmx-template:remove") && !main.contains("build-client"), "{main}");
    assert!(main.contains("unwrap_or_else(|_| \".\".into())"), "APP_ROOT defaults to the app directory: {main}");
    assert!(read(dir, "app/actions/public/entry.js").contains("./pkg/my_app.js"));
    assert!(read(dir, "src/server/tests.rs").contains("my_app.js"));
    assert!(read(dir, "src/server/home_page.rs").contains("unwrap_or(\"My App\")"));
    assert!(read(dir, "src/server/mod.rs").contains("allow_files: vec![\"app/**/public/**\".into()]"));
    assert!(read(dir, "src/prompt_button.rs").contains("rmx::client_entry!(\"PromptButton\")"));
    assert!(read(dir, "src/lib.rs").contains("rmx::entry::<prompt_button::PromptButton>()"));
    assert!(read(dir, "src/routes.rs").contains("#[derive(RouteMap)]"));
    assert!(read(dir, "public/favicon.svg").contains("<svg"));
    assert!(read(dir, "README.md").starts_with("# My App\n"));
    assert!(read(dir, "README.md").contains("target/wasm32-unknown-unknown/wasm-release/my_app.wasm"));
    assert!(read(dir, "AGENTS.md").starts_with("# My App Agent Guide\n"));
    assert!(read(dir, "AGENTS.md").contains("This starter intentionally begins small"));
    assert_eq!(read(dir, ".gitignore"), "/target\n/app/actions/public/pkg\n");
    for file in ["src/lib.rs", "src/main.rs", "src/prompt_button.rs", "src/routes.rs", "src/server/mod.rs", "src/server/home_page.rs", "src/server/tests.rs"] {
        let source = read(dir, file);
        assert!(!source.contains("template_demo") && !source.contains("template-demo"), "{file}");
    }
}

#[test]
fn derives_names_from_the_target_directory() {
    let scratch = Scratch::new("names");
    let (result, _) = new_app(&scratch.0, &["new", "./nested/../cool_new-app"]);
    let app = result.unwrap();
    assert_eq!((app.display_name.as_str(), app.package_name.as_str()), ("Cool New App", "cool-new-app"));
    assert_eq!(app.target_dir, scratch.0.join("cool_new-app"));
    assert_eq!(display_path(&app.target_dir, &scratch.0), "cool_new-app");
    assert!(read(&app.target_dir, "src/main.rs").contains("cool_new_app::routes::ROUTES"));
}

#[test]
fn escapes_app_names_in_the_generated_document_title() {
    let scratch = Scratch::new("quoted");
    let name = r#"Bob's \ "App""#;
    let (result, _) = new_app(&scratch.0, &["new", "quoted-app", "--app-name", name]);
    let dir = result.unwrap().target_dir;
    let home = read(&dir, "src/server/home_page.rs");
    assert!(home.contains(r#"unwrap_or("Bob's \\ \"App\"")"#), "{home}");
}

#[test]
fn rejects_non_empty_target_directories_without_force() {
    let scratch = Scratch::new("non-empty");
    let dir = scratch.0.join("my-app");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.txt"), "keep").unwrap();
    let (result, lines) = new_app(&scratch.0, &["new", "my-app"]);
    let err = result.unwrap_err();
    assert_eq!(err.code, "RMX_TARGET_DIRECTORY_NOT_EMPTY");
    assert_eq!(err.message, format!("Target directory is not empty: {}. Re-run with --force to continue.", dir.display()));
    assert_eq!(lines.last().unwrap(), "\u{2717} Prepare target directory");
    assert!(!dir.join("Cargo.toml").exists());

    let (result, _) = new_app(&scratch.0, &["new", "my-app", "--force"]);
    assert!(result.is_ok());
    assert_eq!(read(&dir, "keep.txt"), "keep");
    assert!(dir.join("Cargo.toml").exists());
}

#[test]
fn rejects_names_that_cannot_become_package_names_before_writing_files() {
    let scratch = Scratch::new("invalid");
    for name in ["!!!", "123-app", "test"] {
        let (result, lines) = new_app(&scratch.0, &["new", "invalid-name", "--app-name", name]);
        let err = result.unwrap_err();
        assert_eq!(err.message, format!("Could not derive a valid package name from {name:?}."));
        assert!(lines.is_empty());
    }
    assert!(!scratch.0.join("invalid-name").exists());
}

#[test]
fn marks_scaffold_generation_as_failed_when_a_file_is_in_the_way() {
    let scratch = Scratch::new("blocked");
    let dir = scratch.0.join("my-app");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app"), "blocking file").unwrap();
    let (result, lines) = new_app(&scratch.0, &["new", "my-app", "--force"]);
    assert_eq!(result.unwrap_err().code, "RMX_INTERNAL_ERROR");
    assert_eq!(lines[1], "\u{2713} Prepare target directory");
    assert_eq!(lines.last().unwrap(), "\u{2717} Generate scaffold files");

    std::fs::write(scratch.0.join("file"), "x").unwrap();
    let (result, _) = new_app(&scratch.0, &["new", "file"]);
    assert_eq!(result.unwrap_err().code, "RMX_TARGET_PATH_NOT_DIRECTORY");
}

// Ports of doctor.test.ts, for rmx's suites (environment, project, client).

struct FakeToolchain {
    targets: Option<Vec<&'static str>>,
    wasm_bindgen: Option<&'static str>,
}

impl doctor::Toolchain for FakeToolchain {
    fn installed_targets(&self) -> Option<Vec<String>> {
        self.targets.as_ref().map(|t| t.iter().map(|s| s.to_string()).collect())
    }

    fn wasm_bindgen_version(&self) -> Option<String> {
        self.wasm_bindgen.map(str::to_string)
    }
}

fn ready() -> FakeToolchain {
    FakeToolchain { targets: Some(vec!["x86_64-pc-windows-msvc", "wasm32-unknown-unknown"]), wasm_bindgen: Some(doctor::WASM_BINDGEN_VERSION) }
}

fn scaffold(scratch: &Scratch) -> PathBuf {
    let (result, _) = new_app(&scratch.0, &["new", "doc-app"]);
    result.unwrap().target_dir
}

#[test]
fn doctor_parses_its_flags() {
    assert_eq!(parse(&args(&["doctor"])), Ok(Invocation::Doctor { json: false, strict: false, fix: false }));
    assert_eq!(parse(&args(&["doctor", "--json", "--strict"])), Ok(Invocation::Doctor { json: true, strict: true, fix: false }));
    assert_eq!(parse(&args(&["doctor", "--strict", "--no-strict"])), Ok(Invocation::Doctor { json: false, strict: false, fix: false }));
    assert_eq!(parse(&args(&["doctor", "--fix"])), Ok(Invocation::Doctor { json: false, strict: false, fix: true }));
    assert_eq!(parse(&args(&["doctor", "--bad-flag"])).unwrap_err().message, "Unknown argument: --bad-flag");
    assert!(DOCTOR_HELP.contains("  --fix        Apply available low-risk fixes\n"));
    assert_eq!(parse(&args(&["doctor", "--help"])), Ok(Invocation::Help(DOCTOR_HELP)));
    assert!(HELP.contains("doctor            Check project health for the current project"));
}

#[test]
fn doctor_finds_no_issues_in_a_new_app_with_a_built_client() {
    let scratch = Scratch::new("doctor-ok");
    let app = scaffold(&scratch);
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::create_dir_all(app.join("app/actions/public/pkg")).unwrap();
    std::fs::write(app.join("app/actions/public/pkg/doc_app_bg.wasm"), "\0asm").unwrap();
    let report = doctor::run_doctor(&app.join("src/server"), &ready());
    assert_eq!(report.project_root.as_deref(), Some(app.as_path()), "found from a nested directory");
    assert_eq!(
        report.render(),
        "\n\u{2022} Checking environment...\n\u{2713} environment\n\n\u{2022} Checking project...\n\u{2713} project\n\n\u{2022} Checking client...\n\u{2713} client\n\nDoctor found no issues.\nSummary: 0 warnings, 0 advice.\n"
    );
}

#[test]
fn doctor_advises_building_the_client_and_rebuilding_it_when_stale() {
    let scratch = Scratch::new("doctor-client");
    let app = scaffold(&scratch);
    let report = doctor::run_doctor(&app, &ready());
    let text = report.render();
    assert!(text.contains("\u{2713} client\n  \u{2022} [ADVICE] The client has not been built: no doc_app_bg.wasm outside target/"), "{text}");
    assert!(text.ends_with("Summary: 0 warnings, 1 advice.\n"), "{text}");

    std::fs::create_dir_all(app.join("app/actions/public/pkg")).unwrap();
    std::fs::write(app.join("app/actions/public/pkg/doc_app_bg.wasm"), "\0asm").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(app.join("src/routes.rs"), read(&app, "src/routes.rs") + "\n").unwrap();
    let text = doctor::run_doctor(&app, &ready()).render();
    assert!(text.contains("[ADVICE] The built client (app/actions/public/pkg/doc_app_bg.wasm) is older than src/; rebuild it."), "{text}");
}

#[test]
fn doctor_skips_later_suites_when_the_environment_warns() {
    let scratch = Scratch::new("doctor-env");
    let report = doctor::run_doctor(&scratch.0, &ready());
    let text = report.render();
    assert!(text.contains("\u{2717} environment\n  \u{2022} [WARN] Could not find Cargo.toml. Run this command inside an rmx project."), "{text}");
    assert!(text.contains("\u{2022} project (skipped: Blocked by environment warnings.)"), "{text}");
    assert!(text.contains("\u{2022} client (skipped: Blocked by environment warnings.)"), "{text}");
    assert_eq!(report.warnings(), 1);

    let app = scaffold(&scratch);
    let no_wasm = FakeToolchain { targets: Some(vec!["x86_64-pc-windows-msvc"]), wasm_bindgen: Some("0.2.100") };
    let text = doctor::run_doctor(&app, &no_wasm).render();
    assert!(text.contains("[WARN] The wasm32-unknown-unknown target is not installed. Run `rustup target add wasm32-unknown-unknown`."), "{text}");
    assert!(text.contains("[WARN] wasm-bindgen 0.2.100 is on PATH, but rmx uses 0.2.129."), "{text}");
    let unknown = FakeToolchain { targets: None, wasm_bindgen: None };
    let report = doctor::run_doctor(&app, &unknown);
    assert_eq!(report.warnings(), 0, "missing tools are advice: {}", report.render());
    assert_eq!(report.findings().count(), 3, "{}", report.render());

    std::fs::write(app.join("Cargo.toml"), read(&app, "Cargo.toml").replace("rmx = {", "other = {")).unwrap();
    let text = doctor::run_doctor(&app, &ready()).render();
    assert!(text.contains("[WARN] Cargo.toml does not declare an rmx dependency."), "{text}");
}

#[test]
fn doctor_checks_the_route_map_and_the_client_crate_type() {
    let scratch = Scratch::new("doctor-project");
    let app = scaffold(&scratch);
    std::fs::write(app.join("Cargo.toml"), read(&app, "Cargo.toml").replace("[\"cdylib\", \"rlib\"]", "[\"rlib\"]")).unwrap();
    let text = doctor::run_doctor(&app, &ready()).render();
    assert!(text.contains("\u{2717} client\n  \u{2022} [WARN] Cargo.toml [lib] crate-type does not include \"cdylib\""), "{text}");

    std::fs::write(app.join("src/routes.rs"), "pub struct Routes;\n").unwrap();
    let report = doctor::run_doctor(&app, &ready());
    let text = report.render();
    assert!(text.contains("\u{2717} project\n  \u{2022} [WARN] Project has no route map"), "{text}");
    assert!(text.contains("\u{2022} client (skipped: Blocked by project warnings.)"), "{text}");
    let json = report.to_json();
    assert!(json.contains(r#""suites":[{"name":"environment","status":"ok","findings":[]}"#), "{json}");
    assert!(json.contains(r#"{"name":"client","status":"skipped","reason":"Blocked by project warnings.","findings":[]}"#), "{json}");
    assert!(json.contains(r#""code":"routes-missing","severity":"warn","suite":"project""#), "{json}");
}

// doctor --fix (doctor.test.ts fix cases and doctor/fixes.test.ts)

fn fix(app: &Path) -> doctor::Report {
    doctor::run_doctor_with(app, &ready(), true).unwrap()
}

fn built(app: &Path) {
    std::fs::create_dir_all(app.join("app/actions/public/pkg")).unwrap();
    std::fs::write(app.join("app/actions/public/pkg/doc_app_bg.wasm"), "\0asm").unwrap();
}

#[test]
fn doctor_fix_applies_no_fixes_to_a_clean_app() {
    let scratch = Scratch::new("doctor-fix-clean");
    let app = scaffold(&scratch);
    std::thread::sleep(std::time::Duration::from_millis(20));
    built(&app);
    let before = read(&app, "Cargo.toml");
    let report = fix(&app);
    let text = report.render();
    assert!(!text.contains("Applied"), "{text}");
    assert!(text.contains("Doctor found no issues."), "{text}");
    assert_eq!(read(&app, "Cargo.toml"), before);
    let json = report.to_json();
    assert!(json.contains(r#""appliedFixes":[],"remainingFindings":[]}"#), "{json}");
}

#[test]
fn doctor_fix_adds_the_rmx_dependency() {
    let scratch = Scratch::new("doctor-fix-dep");
    let app = scaffold(&scratch);
    let manifest = read(&app, "Cargo.toml");
    let rmx_line = manifest.lines().find(|l| l.starts_with("rmx = ")).unwrap().to_string();
    std::fs::write(app.join("Cargo.toml"), manifest.replace(&format!("{rmx_line}\n"), "")).unwrap();
    assert!(doctor::run_doctor(&app, &ready()).render().contains("[WARN] Cargo.toml does not declare an rmx dependency."));

    let report = fix(&app);
    let text = report.render();
    assert!(text.contains("\u{2713} environment\n  Applied fixes:\n    \u{2022} Updated Cargo.toml\n"), "{text}");
    assert!(text.contains("Applied 1 fix.\n"), "{text}");
    let manifest = read(&app, "Cargo.toml");
    assert!(manifest.contains(&format!("[dependencies]\n{}\n", super::rmx_dependency())), "{manifest}");
    assert_eq!(report.warnings(), 0, "{text}");
}

#[test]
fn doctor_fix_creates_the_route_map_and_declares_it() {
    let scratch = Scratch::new("doctor-fix-routes");
    let app = scaffold(&scratch);
    std::fs::remove_file(app.join("src/routes.rs")).unwrap();
    let lib = read(&app, "src/lib.rs").replace("pub mod routes;\n", "");
    std::fs::write(app.join("src/lib.rs"), &lib).unwrap();

    let report = fix(&app);
    let text = report.render();
    assert!(text.contains("\u{2713} project\n  Applied fixes:\n    \u{2022} Created src/routes.rs\n    \u{2022} Updated src/lib.rs\n"), "{text}");
    assert!(read(&app, "src/routes.rs").contains("#[derive(RouteMap)]\npub struct Routes {\n    pub home: Route,\n}"));
    assert!(read(&app, "src/lib.rs").ends_with("\npub mod routes;\n"));
    let json = report.to_json();
    assert!(json.contains(r#"{"code":"routes-missing","kind":"create-file","path":"src/routes.rs","suite":"project"}"#), "{json}");

    // A routes file with only comments is replaced; lib.rs already declares it.
    std::fs::write(app.join("src/routes.rs"), "// TODO: define routes\n\n/* routes coming soon */\n").unwrap();
    let text = fix(&app).render();
    assert!(text.contains("\u{2713} project\n  Applied fixes:\n    \u{2022} Updated src/routes.rs\n\n"), "{text}");
    assert!(read(&app, "src/routes.rs").contains("derive(RouteMap)"));

    // Real code without a route map is left to the developer.
    std::fs::write(app.join("src/routes.rs"), "pub struct Routes;\n").unwrap();
    let report = fix(&app);
    assert!(report.render().contains("\u{2717} project\n  \u{2022} [WARN] Project has no route map"), "{}", report.render());
    assert_eq!(report.applied_fixes().count(), 0);
    assert_eq!(read(&app, "src/routes.rs"), "pub struct Routes;\n");
    let json = report.to_json();
    assert!(json.contains(r#""appliedFixes":[],"remainingFindings":[{"code":"routes-missing""#), "{json}");
}

#[test]
fn doctor_fix_adds_cdylib_to_the_crate_types() {
    let scratch = Scratch::new("doctor-fix-cdylib");
    let app = scaffold(&scratch);
    let manifest = read(&app, "Cargo.toml");
    std::fs::write(app.join("Cargo.toml"), manifest.replace("[\"cdylib\", \"rlib\"]", "[\"rlib\"]")).unwrap();
    let text = fix(&app).render();
    assert!(text.contains("client\n  \u{2022} [ADVICE] The client has not been built"), "{text}");
    assert!(text.contains("  Applied fixes:\n    \u{2022} Updated Cargo.toml\n"), "{text}");
    assert!(read(&app, "Cargo.toml").contains("crate-type = [\"cdylib\", \"rlib\"]"));

    let without_lib = manifest.replace("[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n", "");
    assert_ne!(without_lib, manifest);
    std::fs::write(app.join("Cargo.toml"), without_lib).unwrap();
    fix(&app);
    assert!(read(&app, "Cargo.toml").contains("[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n[dependencies]"), "{}", read(&app, "Cargo.toml"));
}

#[test]
fn colors_help_headings_and_syntax_tokens() {
    // help-text.test.ts: plain without colors, headings bold light blue, tokens light yellow.
    assert_eq!(style_help(HELP, &Styles::new(false)), HELP);
    let text = "Usage:\n  rmx demo [options]\n\nRun the demo command.\n\nOptions:\n  --dir <path>  Print JSON to <file>\n  -h, --help    Show help\n\nExamples:\n  rmx demo --json\n  rmx new ./my-rmx-app --force\n  rmx new ./foo--bar --force\n  rmx dev -- --port 4000\n";
    let out = style_help(text, &Styles::new(true));
    let y = |s: &str| format!("\x1b[93m{s}\x1b[39m");
    assert!(out.starts_with("\x1b[1m\x1b[94mUsage\x1b[39m\x1b[22m:\n"), "{out:?}");
    assert!(out.contains(&format!("  rmx demo {}\n\nRun the demo command.\n\n", y("[options]"))), "{out:?}");
    assert!(out.contains(&format!("  {} {}  Print JSON to <file>\n", y("--dir"), y("<path>"))), "only the label: {out:?}");
    assert!(out.contains(&format!("  {}, {}    Show help\n", y("-h"), y("--help"))), "{out:?}");
    assert!(out.contains(&format!("  rmx demo {}\n", y("--json"))), "{out:?}");
    assert!(out.contains(&format!("  rmx new ./foo--bar {}\n", y("--force"))), "{out:?}");
    assert!(out.contains(&format!("  rmx dev -- {} 4000\n", y("--port"))), "a bare -- is not a flag: {out:?}");
    assert!(!out.contains("Run the \x1b"), "descriptions stay plain");
    // Every help text keeps its words once the colors are stripped.
    for help in [HELP, NEW_HELP, VERSION_HELP, ROUTES_HELP, ASSETS_HELP, DOCTOR_HELP, DEV_HELP, COMPLETION_HELP, TEST_HELP] {
        assert_eq!(strip_sgr(&style_help(help, &Styles::new(true))), help);
    }
}

/// `text` without its `ESC [ ... m` sequences.
fn strip_sgr(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('\x1b') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        rest = rest.find('m').map_or("", |end| &rest[end + 1..]);
    }
    out.push_str(rest);
    out
}

#[test]
fn commands_open_with_the_header_on_a_terminal() {
    // reporter.test.ts: a blank line, then the header only on a TTY.
    assert_eq!(preamble("doctor", &Styles::new(false), true), format!("\nRMX v{} - doctor\n\n", version()));
    assert_eq!(preamble("new", &Styles::new(false), false), "\n");
    // terminal.test.ts: one color per letter when colors are on.
    let colored = preamble("doctor", &Styles::new(true), true);
    assert!(colored.starts_with("\n\x1b[94mR\x1b[39m\x1b[93mM\x1b[39m\x1b[91mX\x1b[39m v"), "{colored:?}");

}

/// A status channel writing to a buffer, and the buffer.
fn status_buffer(styles: Styles, live: bool) -> (status::Status, Arc<Mutex<Vec<u8>>>) {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    (status::Status::new(buffer.clone(), styles, live), buffer)
}

fn text_of(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
}

#[test]
fn doctor_reports_each_suite_as_it_runs() {
    let scratch = Scratch::new("doctor-progress");
    let app = scaffold(&scratch);
    std::fs::write(app.join("src/routes.rs"), "// nothing yet\n").unwrap();
    for styles in [Styles::new(false), Styles::new(true)] {
        let (mut status, buffer) = status_buffer(styles, false);
        let mut running = None;
        let report = doctor::run_doctor_reporting(&app, &ready(), false, &mut |event| report_doctor(&mut status, &mut running, event, &styles)).unwrap();
        status.text(&report.summary());
        drop(status);
        // Streamed, the report reads like the one rendered at the end, without its preamble.
        assert_eq!(format!("\n{}", text_of(&buffer)), report.render_styled(&styles));
        let plain = report.render();
        assert!(plain.contains("\u{2717} project\n") && plain.contains("\u{2022} client (skipped: Blocked by project warnings.)\n"), "{plain}");
    }
}

#[test]
fn status_redraws_a_running_step_on_a_terminal() {
    // reporter.test.ts: live frames on an ANSI terminal, stable lines elsewhere.
    let (status, buffer) = status_buffer(Styles::new(true), true);
    let mut status = status.with_interval(std::time::Duration::from_millis(5));
    status.line("\u{2022} Checking environment...");
    std::thread::sleep(std::time::Duration::from_millis(40));
    status.line("\u{2713} Checking environment");
    drop(status);
    let out = text_of(&buffer);
    let frame = |dots: &str| format!("\r\x1b[2K\x1b[90m\u{2022} Checking environment{dots}\x1b[39m");
    assert!(out.starts_with(&frame(".")), "{out:?}");
    assert!(out.contains(&frame("..")) || out.contains(&frame("...")), "{out:?}");
    assert!(out.ends_with("\r\x1b[2K\x1b[92m\u{2713}\x1b[39m \x1b[90mChecking environment\x1b[39m\n"), "{out:?}");

    let (mut status, buffer) = status_buffer(Styles::new(false), false);
    status.line("\u{2022} Checking project...");
    status.line("\u{2717} Checking project");
    status.line("\u{2022} client (skipped: Blocked by project warnings.)");
    drop(status);
    assert_eq!(text_of(&buffer), "\u{2022} Checking project...\n\u{2717} Checking project\n\u{2022} client (skipped: Blocked by project warnings.)\n");
}

#[test]
fn restores_terminal_formatting_where_ansi_works() {
    // terminal.test.ts restoreTerminalFormatting: stdout first, then stderr, not on TERM=dumb.
    assert!(can_use_ansi(Some("xterm-256color"), true) && can_use_ansi(None, true));
    assert!(!can_use_ansi(Some("dumb"), true) && !can_use_ansi(Some("xterm"), false));
    assert_eq!(restore_target(true, true), Some(Stream::Stdout));
    assert_eq!(restore_target(false, true), Some(Stream::Stderr));
    assert_eq!(restore_target(false, false), None);
}

#[test]
fn doctor_fails_on_warnings_under_strict_or_fix() {
    assert_eq!(doctor_exit_code(false, false, 2), 0);
    assert_eq!(doctor_exit_code(true, false, 1), 1);
    assert_eq!(doctor_exit_code(false, true, 1), 1);
    assert_eq!(doctor_exit_code(true, true, 0), 0);
}

#[test]
fn doctor_fix_skips_existing_files_and_refuses_paths_outside_the_app() {
    use doctor::{apply_fix_plans, FixKind, FixPlan};
    let scratch = Scratch::new("doctor-fix-plans");
    let app = scratch.0.join("app");
    std::fs::create_dir_all(app.join("src")).unwrap();
    std::fs::write(app.join("src/routes.rs"), "kept\n").unwrap();
    let plan = |kind, path: &str| FixPlan { code: "routes-missing", suite: "project", kind, path: path.into(), contents: "new\n".into() };

    let applied = apply_fix_plans(&app, &[plan(FixKind::CreateFile, "src/routes.rs")]).unwrap();
    assert!(applied.is_empty(), "a skipped creation is not an applied fix");
    assert_eq!(read(&app, "src/routes.rs"), "kept\n");
    let applied = apply_fix_plans(&app, &[plan(FixKind::UpdateFile, "src/a.rs"), plan(FixKind::UpdateFile, "src/a.rs"), plan(FixKind::CreateFile, "src/b.rs")]).unwrap();
    assert_eq!(applied.len(), 2, "duplicate plans run once");
    assert_eq!(read(&app, "src/a.rs"), "new\n");

    let err = apply_fix_plans(&app, &[plan(FixKind::CreateFile, "../escape.rs")]).unwrap_err();
    assert_eq!(err, "Doctor fix path resolves outside the app root: ../escape.rs");
    assert!(!scratch.0.join("escape.rs").exists());
    assert!(apply_fix_plans(&app, &[plan(FixKind::CreateFile, "src/../../escape.rs")]).is_err());
    let absolute = scratch.0.join("abs.rs").display().to_string();
    assert!(apply_fix_plans(&app, &[plan(FixKind::CreateFile, &absolute)]).is_err());
    assert!(!scratch.0.join("abs.rs").exists());

    // Through symlinks (where this machine allows creating them).
    let external = scratch.0.join("external");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("Cargo.toml"), "external\n").unwrap();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&external, app.join("linked")).is_ok() && std::os::windows::fs::symlink_file(external.join("Cargo.toml"), app.join("Linked.toml")).is_ok();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&external, app.join("linked")).is_ok() && std::os::unix::fs::symlink(external.join("Cargo.toml"), app.join("Linked.toml")).is_ok();
    if linked {
        for (kind, path) in [(FixKind::CreateFile, "linked/routes.rs"), (FixKind::CreateDirectory, "linked/actions"), (FixKind::UpdateFile, "Linked.toml")] {
            let err = apply_fix_plans(&app, &[plan(kind, path)]).unwrap_err();
            assert!(err.contains("outside the app root"), "{err}");
        }
        assert!(!external.join("routes.rs").exists() && !external.join("actions").exists());
        assert_eq!(read(&external, "Cargo.toml"), "external\n");
    } else {
        eprintln!("symlinks unavailable: skipped the symlink cases");
    }
}

// rmx dev

#[test]
fn dev_parses_app_arguments_after_a_separator() {
    assert_eq!(parse(&args(&["dev"])), Ok(Invocation::Dev(vec![])));
    assert_eq!(parse(&args(&["dev", "--", "--port", "4000"])), Ok(Invocation::Dev(args(&["--port", "4000"]))));
    assert_eq!(parse(&args(&["dev", "--help"])), Ok(Invocation::Help(DEV_HELP)));
    assert_eq!(parse(&args(&["dev", "--", "--help"])), Ok(Invocation::Dev(args(&["--help"]))), "after -- it is the app's");
    assert_eq!(parse(&args(&["dev", "--watch"])).unwrap_err().message, "Unknown argument: --watch");
    assert_eq!(parse(&args(&["dev", "extra"])).unwrap_err().message, "Unexpected extra argument: extra");
}

#[test]
fn dev_plans_the_client_and_server_builds() {
    let scratch = Scratch::new("dev-plan");
    let app = scaffold(&scratch);
    let plan = dev::DevPlan::for_app(&app.join("src")).unwrap();
    assert_eq!(plan.root, app);
    assert_eq!(plan.package, "doc-app");
    assert_eq!(plan.client_out, Some(app.join("app/actions/public/pkg")));
    let commands: Vec<String> = plan.build_commands("wb").iter().map(|c| c.join(" ")).collect();
    assert_eq!(commands[0], "cargo build --lib --target wasm32-unknown-unknown");
    assert!(commands[1].starts_with("wb --target web --no-typescript --out-dir "), "{}", commands[1]);
    assert!(commands[1].ends_with("doc_app.wasm"), "{}", commands[1]);
    assert_eq!(commands[2], "cargo build --bins");
    assert_ne!(plan.server_binary(), plan.running_binary(), "the server runs from a copy so rebuilds can replace it");

    // An existing client build decides the output directory.
    std::fs::create_dir_all(app.join("public/pkg")).unwrap();
    std::fs::write(app.join("public/pkg/doc_app_bg.wasm"), "").unwrap();
    assert_eq!(dev::DevPlan::for_app(&app).unwrap().client_out, Some(app.join("public/pkg")));

    // A server-only app builds only the server.
    std::fs::write(app.join("src/lib.rs"), "pub mod routes;\n").unwrap();
    let plan = dev::DevPlan::for_app(&app).unwrap();
    assert_eq!(plan.client_out, None);
    assert_eq!(plan.build_commands("wb").len(), 1);
    assert!(dev::DevPlan::for_app(&std::env::temp_dir().join("rmx-cli-no-such-app")).is_err());
}

#[test]
fn dev_sees_source_changes_but_not_its_own_client_output() {
    let scratch = Scratch::new("dev-watch");
    let app = scaffold(&scratch);
    let plan = dev::DevPlan::for_app(&app).unwrap();
    let before = plan.snapshot();
    assert!(before.contains_key(&app.join("src/routes.rs")) && before.contains_key(&app.join("Cargo.toml")));
    std::fs::create_dir_all(app.join("app/actions/public/pkg")).unwrap();
    std::fs::write(app.join("app/actions/public/pkg/doc_app_bg.wasm"), "").unwrap();
    assert_eq!(plan.snapshot(), before, "client output is not a change");
    std::fs::write(app.join("src/new_module.rs"), "").unwrap();
    assert_ne!(plan.snapshot(), before);
}

// Ports of completion.test.ts and commands/completion.test.ts.

fn complete(line: &[&str]) -> String {
    // The word being completed is the last one ("" after a trailing space).
    let mut list = vec!["completion", "--", ""];
    let index = (line.len() - 1).to_string();
    list[2] = &index;
    list.extend(line);
    match parse(&args(&list)).unwrap() {
        Invocation::Print(out) => out,
        other => panic!("{other:?}"),
    }
}

#[test]
fn completion_prints_the_script_for_bash_and_zsh() {
    let Invocation::Print(bash) = parse(&args(&["completion", "bash"])).unwrap() else { panic!() };
    let Invocation::Print(zsh) = parse(&args(&["completion", "zsh"])).unwrap() else { panic!() };
    assert_eq!(bash, zsh);
    assert!(bash.starts_with("###-begin-rmx-completion-###\n"));
    assert!(bash.contains("complete -o default -F _rmx_completion rmx") && bash.contains("compdef _rmx_completion rmx"));
    assert!(bash.contains(r#"output=$(rmx completion -- "$cword" "${words[@]}" 2>/dev/null)"#), "{bash}");
    assert_eq!(parse(&args(&["completion", "fish"])).unwrap_err().message, "Unknown completion shell: fish");
    assert_eq!(parse(&args(&["completion"])), Ok(Invocation::Help(COMPLETION_HELP)));
    assert!(HELP.contains("completion        Print shell completion scripts for rmx"));
}

#[test]
fn completion_offers_commands_subcommands_flags_and_files() {
    let commands = "assets\ncompletion\ndev\ndoctor\nhelp\nnew\nroutes\ntest\nversion\n";
    assert_eq!(complete(&["rmx", ""]), format!("mode:values\n{commands}-h\n--help\n--no-color\n-v\n--version\n"));
    assert_eq!(complete(&["rmx", "-"]), "mode:values\n-h\n--help\n--no-color\n-v\n--version\n");
    assert_eq!(complete(&["rmx", "d"]), "mode:values\ndev\ndoctor\n");
    assert_eq!(complete(&["rmx", "--v"]), "mode:values\n--version\n");
    assert_eq!(complete(&["rmx", "--version", ""]), "mode:values\n");
    assert_eq!(complete(&["rmx", "help", ""]), format!("mode:values\n{commands}-h\n--help\n--no-color\n"));
    assert_eq!(complete(&["rmx", "help", "ro"]), "mode:values\nroutes\n");
    assert_eq!(complete(&["rmx", "completion", ""]), "mode:values\nbash\nzsh\n-h\n--help\n--no-color\n");
    assert_eq!(complete(&["rmx", "assets", ""]), "mode:values\ninspect\n-h\n--help\n--no-color\n");
    assert_eq!(complete(&["rmx", "assets", "inspect", "app/"]), "mode:files\n");
    assert_eq!(complete(&["rmx", "assets", "inspect", "-"]), "mode:values\n-h\n--help\n--no-color\n");
    assert_eq!(complete(&["rmx", "new", ""]), "mode:files\n");
    assert_eq!(complete(&["rmx", "new", "my-app", "--"]), "mode:values\n--app-name\n--force\n--help\n--no-color\n");
    assert_eq!(complete(&["rmx", "new", "my-app", "--app-name", ""]), "mode:none\n");
    assert_eq!(complete(&["rmx", "new", "--app-name", "X", ""]), "mode:files\n", "the name is not the target");
    assert_eq!(complete(&["rmx", "doctor", "--json", "--"]), "mode:values\n--fix\n--strict\n--no-strict\n--help\n--no-color\n");
    // A used --no-color is not offered again, before or after the command.
    assert_eq!(complete(&["rmx", "--no-color", "doctor", "--"]), "mode:values\n--fix\n--json\n--strict\n--no-strict\n--help\n");
    assert_eq!(complete(&["rmx", "--no-color", ""]), format!("mode:values\n{commands}-h\n--help\n-v\n--version\n"));
    assert_eq!(complete(&["rmx", "routes", "--no-color", "--n"]), "mode:values\n--no-headers\n");
    assert_eq!(complete(&["rmx", "routes", "--t"]), "mode:values\n--table\n");
    assert_eq!(complete(&["rmx", "test", ""]), "mode:values\n");
    assert_eq!(complete(&["rmx", "routes", "--help", ""]), "mode:values\n");
}

#[test]
fn progress_lines_use_the_reporter_colors() {
    let on = Styles::new(true);
    assert_eq!(style_progress("\u{2022} Generate scaffold files...", &on), "\x1b[90m\u{2022} Generate scaffold files...\x1b[39m");
    assert_eq!(style_progress("\u{2713} Generate scaffold files", &on), "\x1b[92m\u{2713}\x1b[39m \x1b[90mGenerate scaffold files\x1b[39m");
    assert_eq!(style_progress("\u{2717} Finalize Cargo.toml", &on), "\x1b[91m\u{2717}\x1b[39m \x1b[90mFinalize Cargo.toml\x1b[39m");
    assert_eq!(style_progress("Created App", &on), "Created App");
    let off = Styles::new(false);
    assert_eq!(style_progress("\u{2713} Generate scaffold files", &off), "\u{2713} Generate scaffold files");
}

#[test]
fn doctor_colors_marks_and_warnings_only_when_enabled() {
    use doctor::{Finding, Report, Severity, Suite, SuiteStatus};
    let report = Report {
        project_root: None,
        suites: vec![Suite {
            name: "environment",
            status: SuiteStatus::Issues,
            findings: vec![Finding { code: "x", severity: Severity::Warn, message: "Missing target".into() }, Finding { code: "y", severity: Severity::Advice, message: "Consider it".into() }],
            applied_fixes: Vec::new(),
        }],
        fixed: false,
    };
    let colored = report.render_styled(&Styles::new(true));
    assert!(colored.contains("\x1b[91m\u{2717}\x1b[39m \x1b[90menvironment\x1b[39m"), "{colored:?}");
    assert!(colored.contains("  \u{2022} \x1b[93m[WARN] Missing target\x1b[39m\n"), "{colored:?}");
    assert!(colored.contains("  \u{2022} [ADVICE] Consider it\n"), "{colored:?}");
    assert_eq!(rmx_terminal::strip_ansi(&colored), report.render());
}
