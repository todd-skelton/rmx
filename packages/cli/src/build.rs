//! `rsmix build`: the production build. Remix's template has no build step (`npm start` runs
//! the TypeScript through Node), but an rsmix app compiles its client ahead of time, so this
//! is the step `rsmix dev` does for development, done for production: the client with the
//! size-optimized `wasm-release` profile (`--release` when the app has no such profile),
//! `wasm-bindgen`, and `wasm-opt -Oz` when it can be had, then `cargo build --release` for the
//! server. Start the result with `cargo run --release`.

use std::path::Path;
use std::process::Command;

use super::dev::DevPlan;
use super::doctor::read;
use super::{toolchain, CliError};

/// The `wasm-opt` flags for the features the client's code uses (wasm-bindgen's output and
/// Rust's default wasm32 features).
const WASM_OPT_FLAGS: [&str; 6] = [
    "-Oz",
    "--enable-bulk-memory",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-mutable-globals",
    "--enable-reference-types",
];

/// The commands that build `plan`'s app for production, in order. `manifest` is its
/// Cargo.toml, which says whether there is a `wasm-release` profile.
pub fn build_commands(plan: &DevPlan, manifest: &str, wasm_bindgen: &str, wasm_opt: Option<&str>) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    if let Some(out) = &plan.client_out {
        let (profile_args, profile_dir) = if manifest.contains("[profile.wasm-release]") {
            (vec!["--profile", "wasm-release"], "wasm-release")
        } else {
            (vec!["--release"], "release")
        };
        let ident = plan.package.replace('-', "_");
        let wasm = plan.root.join(format!("target/{}/{profile_dir}/{ident}.wasm", toolchain::WASM_TARGET));
        let mut cargo: Vec<String> = ["cargo", "build", "--lib", "--target", toolchain::WASM_TARGET].map(String::from).to_vec();
        cargo.extend(profile_args.into_iter().map(String::from));
        commands.push(cargo);
        commands.push(vec![
            wasm_bindgen.into(),
            "--target".into(),
            "web".into(),
            "--no-typescript".into(),
            "--out-dir".into(),
            out.display().to_string(),
            wasm.display().to_string(),
        ]);
        if let Some(wasm_opt) = wasm_opt {
            let bg = out.join(format!("{ident}_bg.wasm")).display().to_string();
            let mut opt = vec![wasm_opt.to_string()];
            opt.extend(WASM_OPT_FLAGS.map(String::from));
            opt.extend(["-o".into(), bg.clone(), bg]);
            commands.push(opt);
        }
    }
    commands.push(["cargo", "build", "--release", "--bins"].map(String::from).to_vec());
    commands
}

/// Build the app containing `cwd` for production, reporting each step on stderr.
pub fn run_build(cwd: &Path) -> Result<(), CliError> {
    let plan = DevPlan::for_app(cwd)?;
    let manifest = read(&plan.root.join("Cargo.toml"));
    let mut say = |line: &str| eprintln!("rsmix build: {line}");
    let (wasm_bindgen, wasm_opt) = match plan.client_out {
        Some(_) => {
            toolchain::ensure_wasm_target(&mut say)?;
            let wasm_bindgen = toolchain::wasm_bindgen(&mut say)?;
            (wasm_bindgen.display().to_string(), toolchain::wasm_opt(&mut say).map(|p| p.display().to_string()))
        }
        None => (String::new(), None),
    };
    for argv in build_commands(&plan, &manifest, &wasm_bindgen, wasm_opt.as_deref()) {
        let program = Path::new(&argv[0]).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        say(&format!("{program} {}", argv[1..].iter().take(4).cloned().collect::<Vec<_>>().join(" ")));
        match Command::new(&argv[0]).args(&argv[1..]).current_dir(&plan.root).status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                return Err(CliError {
                    code: "RMX_BUILD_FAILED",
                    title: "Build failed",
                    message: format!("{program} exited with {status}."),
                    fix: None,
                    show_help: false,
                })
            }
            Err(e) => return Err(CliError::internal(format!("Could not run {}: {e}", argv[0]))),
        }
    }
    if let Some(out) = &plan.client_out {
        let wasm = out.join(format!("{}_bg.wasm", plan.package.replace('-', "_")));
        if let Ok(meta) = std::fs::metadata(&wasm) {
            say(&format!("client: {} ({:.1} KiB)", wasm.display(), meta.len() as f64 / 1024.0));
        }
    }
    say("done; start it with `cargo run --release`");
    Ok(())
}