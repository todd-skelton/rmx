//! `rsmix dev` (Remix's `npm run dev` / `npm run hmr`): build the client and the server, run
//! the app, and rebuild and restart it when a source file changes. Open pages reload through
//! `rsmix::middleware::live_reload`, which the app installs and `rsmix dev` switches on with
//! `RSMIX_DEV_LIVE_RELOAD`.
//!
//! Changes are found by polling modification times under `src/` and `app/` (except the
//! client output) and `Cargo.toml`. A failed build leaves the running server alone. The client
//! is built with the dev profile for speed; `wasm-bindgen` comes from `RSMIX_WASM_BINDGEN` or
//! `PATH`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, SystemTime};

use super::doctor::{find_file, package_name, read, rust_sources};
use super::CliError;

/// What `rsmix dev` builds and runs for an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevPlan {
    pub root: PathBuf,
    pub package: String,
    /// Where `wasm-bindgen` writes the client, when the app has one.
    pub client_out: Option<PathBuf>,
}

impl DevPlan {
    /// The plan for the app containing `cwd`: its package, and the client output directory
    /// (where a `<crate>_bg.wasm` already is, else `app/actions/public/pkg`) when the app
    /// has a browser entry.
    pub fn for_app(cwd: &Path) -> Result<DevPlan, CliError> {
        let root = cwd
            .ancestors()
            .find(|dir| read(&dir.join("Cargo.toml")).contains("[package]"))
            .ok_or_else(|| CliError::internal("Could not find Cargo.toml. Run this command inside an rsmix project.".into()))?
            .to_path_buf();
        let package = package_name(&read(&root.join("Cargo.toml")));
        let has_client = rust_sources(&root.join("src")).iter().any(|(_, source)| source.contains("wasm_bindgen(start)"));
        let client_out = has_client.then(|| {
            let wasm = format!("{}_bg.wasm", package.replace('-', "_"));
            find_file(&root, &wasm).and_then(|f| f.parent().map(Path::to_path_buf)).unwrap_or_else(|| root.join("app/actions/public/pkg"))
        });
        Ok(DevPlan { root, package, client_out })
    }

    /// The commands that build the app, in order.
    pub fn build_commands(&self, wasm_bindgen: &str) -> Vec<Vec<String>> {
        let mut commands = Vec::new();
        if let Some(out) = &self.client_out {
            let wasm = self.root.join(format!("target/wasm32-unknown-unknown/debug/{}.wasm", self.package.replace('-', "_")));
            commands.push(["cargo", "build", "--lib", "--target", "wasm32-unknown-unknown"].map(String::from).to_vec());
            commands.push(vec![
                wasm_bindgen.into(),
                "--target".into(),
                "web".into(),
                "--no-typescript".into(),
                "--out-dir".into(),
                out.display().to_string(),
                wasm.display().to_string(),
            ]);
        }
        commands.push(["cargo", "build", "--bins"].map(String::from).to_vec());
        commands
    }

    /// The server binary that `cargo build` produces.
    pub fn server_binary(&self) -> PathBuf {
        self.root.join("target/debug").join(format!("{}{}", self.package, std::env::consts::EXE_SUFFIX))
    }

    /// The copy of the server binary that runs, so a rebuild can replace the original while
    /// the server is up (Windows locks running executables).
    pub fn running_binary(&self) -> PathBuf {
        self.root.join("target/rsmix-dev").join(format!("{}{}", self.package, std::env::consts::EXE_SUFFIX))
    }

    /// Modification times of the files a rebuild depends on.
    pub fn snapshot(&self) -> BTreeMap<PathBuf, SystemTime> {
        let mut files = BTreeMap::new();
        for dir in ["src", "app", "public"] {
            collect(&self.root.join(dir), self.client_out.as_deref(), &mut files);
        }
        let manifest = self.root.join("Cargo.toml");
        if let Ok(modified) = std::fs::metadata(&manifest).and_then(|m| m.modified()) {
            files.insert(manifest, modified);
        }
        files
    }
}

fn collect(path: &Path, skip: Option<&Path>, files: &mut BTreeMap<PathBuf, SystemTime>) {
    if skip.is_some_and(|skip| path == skip) {
        return;
    }
    let Ok(meta) = std::fs::metadata(path) else { return };
    if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            collect(&entry.path(), skip, files);
        }
    } else if let Ok(modified) = meta.modified() {
        files.insert(path.to_path_buf(), modified);
    }
}

fn run_build(plan: &DevPlan, wasm_bindgen: &str) -> bool {
    for argv in plan.build_commands(wasm_bindgen) {
        let status = Command::new(&argv[0]).args(&argv[1..]).current_dir(&plan.root).status();
        match status {
            Ok(status) if status.success() => {}
            Ok(_) => return false,
            Err(e) => {
                eprintln!("rsmix dev: could not run {}: {e}", argv[0]);
                return false;
            }
        }
    }
    true
}

fn start_server(plan: &DevPlan, args: &[String]) -> Option<Child> {
    let binary = plan.running_binary();
    let copied = binary.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::copy(plan.server_binary(), &binary));
    if let Err(e) = copied {
        eprintln!("rsmix dev: could not copy {} to {}: {e}", plan.server_binary().display(), binary.display());
        return None;
    }
    match Command::new(&binary).args(args).current_dir(&plan.root).env("RSMIX_DEV_LIVE_RELOAD", "1").spawn() {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!("rsmix dev: could not start {}: {e}", binary.display());
            None
        }
    }
}

fn stop(server: &mut Option<Child>) {
    if let Some(mut child) = server.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Run the dev loop for the app containing `cwd` until interrupted. `args` go to the app.
pub fn run_dev(cwd: &Path, args: &[String]) -> Result<(), CliError> {
    let plan = DevPlan::for_app(cwd)?;
    let wasm_bindgen = std::env::var("RSMIX_WASM_BINDGEN").unwrap_or_else(|_| "wasm-bindgen".into());
    let mut server = None;
    let mut built = plan.snapshot();
    if run_build(&plan, &wasm_bindgen) {
        server = start_server(&plan, args);
    } else {
        eprintln!("rsmix dev: build failed; waiting for changes");
    }
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let now = plan.snapshot();
        if now == built {
            continue;
        }
        // Let a burst of saves settle before building.
        std::thread::sleep(Duration::from_millis(100));
        built = plan.snapshot();
        eprintln!("rsmix dev: change detected, rebuilding");
        if run_build(&plan, &wasm_bindgen) {
            stop(&mut server);
            server = start_server(&plan, args);
        } else {
            eprintln!("rsmix dev: build failed; the previous server keeps running");
        }
    }
}
