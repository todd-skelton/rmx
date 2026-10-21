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
    assert_eq!(parse(&[]), Ok(Invocation::Print(HELP.into())));
    assert_eq!(parse(&args(&["--help"])), Ok(Invocation::Print(HELP.into())));
    assert!(HELP.contains("new <name>        Create a new rmx project"));
    assert_eq!(parse(&args(&["help", "new"])), Ok(Invocation::Print(NEW_HELP.into())));
    assert_eq!(parse(&args(&["new"])), Ok(Invocation::Print(NEW_HELP.into())), "new without arguments prints its help");
    assert_eq!(parse(&args(&["new", "--help"])), Ok(Invocation::Print(NEW_HELP.into())));
    assert!(NEW_HELP.contains("Create a new rmx project in the target directory."));
    let version = format!("{}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(parse(&args(&["version"])), Ok(Invocation::Print(version.clone())));
    assert_eq!(parse(&args(&["-v"])), Ok(Invocation::Print(version.clone())));
    assert_eq!(parse(&args(&["--", "--version"])), Ok(Invocation::Print(version)));
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
    assert_eq!(parse(&args(&["test", "--help"])), Ok(Invocation::Print(TEST_HELP.into())));
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
