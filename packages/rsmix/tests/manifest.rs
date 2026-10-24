//! The parts of Remix's `packages/remix/manifest.test.ts` that apply to a Cargo workspace:
//! every library crate is distributed through `rsmix`, under a module named after it, and the
//! crate docs list every module. Remix's README mirrors, guide copies, side-effect
//! declarations and package.json export checks are npm tooling with no counterpart.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `packages/*` workspace members' package names (`rsmix-fetch-router`, ...).
fn library_crates() -> BTreeSet<String> {
    let workspace = read(root().join("Cargo.toml"));
    let members = workspace.split("members = [").nth(1).and_then(|s| s.split(']').next()).expect("workspace members");
    members
        .split(',')
        .map(|m| m.trim().trim_matches('"'))
        .filter(|m| m.starts_with("packages/"))
        .map(|dir| {
            let manifest = read(root().join(dir).join("Cargo.toml"));
            let name = manifest.lines().find_map(|l| l.strip_prefix("name = ")).expect("package name");
            name.trim_matches('"').to_string()
        })
        .collect()
}

/// The crate's path under `rsmix`, like Remix's subpath exports: `rsmix-file-storage` is
/// `rsmix::file_storage` and `rsmix-static-middleware` is `rsmix::middleware::static_files`
/// (`remix/middleware/static`; `static` is a keyword).
fn module_name(package: &str) -> String {
    let name = package.strip_prefix("rsmix-").expect("rsmix- prefix");
    match name {
        "fetch-router" => "router".into(),
        "static-middleware" => "middleware::static_files".into(),
        _ => match name.strip_suffix("-middleware") {
            Some(middleware) => format!("middleware::{}", middleware.replace('-', "_")),
            None => name.replace('-', "_"),
        },
    }
}

/// The last segment of [`module_name`]: the name a crate is re-exported as.
fn export_name(package: &str) -> String {
    module_name(package).rsplit("::").next().expect("a module name").to_string()
}

/// Crates that `rsmix` exposes another way.
const NOT_MODULES: [(&str, &str); 3] = [
    ("rsmix", "the umbrella crate itself"),
    ("rsmix-component", "re-exported at the root (`remix/component`)"),
    ("rsmix-macros", "`rsx!`, re-exported through rsmix-component"),
];

// manifest.test.ts: every public @remix-run/* workspace package is referenced in the manifest
#[test]
fn every_library_crate_is_a_dependency_of_rsmix() {
    let manifest = read(root().join("packages/rsmix/Cargo.toml"));
    let missing: Vec<_> = library_crates()
        .into_iter()
        .filter(|name| name != "rsmix" && name != "rsmix-macros")
        .filter(|name| !manifest.lines().any(|l| l.starts_with(&format!("{name}.workspace")) || l.starts_with(&format!("{name} = "))))
        .collect();
    assert!(missing.is_empty(), "not dependencies of rsmix: {missing:?}");
}

// manifest.test.ts: every manifest entry has a valid remix path format, and all exports of
// every referenced package are covered
#[test]
fn every_library_crate_is_re_exported_under_its_own_name() {
    let lib = read(root().join("packages/rsmix/src/lib.rs"));
    assert!(lib.contains("pub use rsmix_component::*;"), "the component model is at the root");
    let component = read(root().join("packages/component/src/lib.rs"));
    assert!(component.contains("rsmix_macros::"), "rsmix-component re-exports the macros");
    let missing: Vec<_> = library_crates()
        .into_iter()
        .filter(|name| !NOT_MODULES.iter().any(|(n, _)| n == name))
        .filter(|name| {
            !lib.contains(&format!("{} as {}", name.replace('-', "_"), export_name(name)))
        })
        .collect();
    assert!(missing.is_empty(), "not re-exported as rsmix::<name>: {missing:?}");
}

// manifest.test.ts: generates package index links (the crate docs' table lists every module)
#[test]
fn the_crate_docs_list_every_module() {
    let lib = read(root().join("packages/rsmix/src/lib.rs"));
    let docs: String = lib.lines().filter_map(|l| l.strip_prefix("//!")).collect::<Vec<_>>().join("\n");
    let missing: Vec<_> = library_crates()
        .into_iter()
        .filter(|name| !NOT_MODULES.iter().any(|(n, _)| n == name))
        .map(|name| module_name(&name))
        .filter(|module| !docs.contains(&format!("rsmix::{module}")))
        .collect();
    assert!(missing.is_empty(), "missing from the rsmix crate docs: {missing:?}");
}
