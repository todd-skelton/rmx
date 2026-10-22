//! The parts of Remix's `packages/remix/manifest.test.ts` that apply to a Cargo workspace:
//! every library crate is distributed through `rmx`, under a module named after it, and the
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

/// The `packages/*` workspace members' package names (`rmx-router`, ...).
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

/// `rmx-file-storage` is `rmx::file_storage`.
fn module_name(package: &str) -> String {
    package.strip_prefix("rmx-").expect("rmx- prefix").replace('-', "_")
}

/// Crates that `rmx` exposes another way.
const NOT_MODULES: [(&str, &str); 3] = [
    ("rmx", "the umbrella crate itself"),
    ("rmx-component", "re-exported at the root (`remix/component`)"),
    ("rmx-macros", "`rsx!`, re-exported through rmx-component"),
];

// manifest.test.ts: every public @remix-run/* workspace package is referenced in the manifest
#[test]
fn every_library_crate_is_a_dependency_of_rmx() {
    let manifest = read(root().join("packages/rmx/Cargo.toml"));
    let missing: Vec<_> = library_crates()
        .into_iter()
        .filter(|name| name != "rmx" && name != "rmx-macros")
        .filter(|name| !manifest.lines().any(|l| l.starts_with(&format!("{name}.workspace")) || l.starts_with(&format!("{name} = "))))
        .collect();
    assert!(missing.is_empty(), "not dependencies of rmx: {missing:?}");
}

// manifest.test.ts: every manifest entry has a valid remix path format, and all exports of
// every referenced package are covered
#[test]
fn every_library_crate_is_re_exported_under_its_own_name() {
    let lib = read(root().join("packages/rmx/src/lib.rs"));
    assert!(lib.contains("pub use rmx_component::*;"), "the component model is at the root");
    let component = read(root().join("packages/component/src/lib.rs"));
    assert!(component.contains("rmx_macros::"), "rmx-component re-exports the macros");
    let missing: Vec<_> = library_crates()
        .into_iter()
        .filter(|name| !NOT_MODULES.iter().any(|(n, _)| n == name))
        .filter(|name| {
            let module = module_name(name);
            !lib.contains(&format!("{} as {module}", name.replace('-', "_")))
        })
        .collect();
    assert!(missing.is_empty(), "not re-exported as rmx::<name>: {missing:?}");
}

// manifest.test.ts: generates package index links (the crate docs' table lists every module)
#[test]
fn the_crate_docs_list_every_module() {
    let lib = read(root().join("packages/rmx/src/lib.rs"));
    let docs: String = lib.lines().filter_map(|l| l.strip_prefix("//!")).collect::<Vec<_>>().join("\n");
    let missing: Vec<_> = library_crates()
        .into_iter()
        .filter(|name| !NOT_MODULES.iter().any(|(n, _)| n == name))
        .map(|name| module_name(&name))
        .filter(|module| !docs.contains(&format!("rmx::{module}")))
        .collect();
    assert!(missing.is_empty(), "missing from the rmx crate docs: {missing:?}");
}
