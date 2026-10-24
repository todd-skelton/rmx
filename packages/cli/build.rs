//! Where `rsmix new` reads the starter app from, like Remix's `resolveTemplateDirectory`:
//! the repository's `template/` in a checkout, else the copy that
//! `scripts/sync-cli-template.ps1` puts in this package before it is published (Remix's
//! `prepack`). Also tells the CLI whether it was built from a checkout, so new apps depend on
//! that checkout's `packages/rsmix` by path instead of the published crate.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let repo_template = manifest_dir.join("../../template");
    let packaged_template = manifest_dir.join("template");
    // A path that does not exist would rerun this script on every build.
    for dir in [&repo_template, &packaged_template].into_iter().filter(|d| d.is_dir()) {
        println!("cargo:rerun-if-changed={}", dir.display());
    }
    println!("cargo:rerun-if-changed=build.rs");

    let template = if is_template(&repo_template) {
        repo_template
    } else if is_template(&packaged_template) {
        packaged_template
    } else {
        panic!("rsmix-cli: no starter template. Run scripts/sync-cli-template.ps1 before packaging.");
    };
    println!("cargo:rustc-env=RSMIX_TEMPLATE_DIR={}", slashes(&template));

    // `cargo install --git` builds in cargo's git cache, which cargo may clean; apps made by that
    // CLI depend on the published crate instead.
    let checkout = manifest_dir.join("../rsmix");
    let in_cargo_cache = slashes(&manifest_dir).contains("/git/checkouts/");
    if !in_cargo_cache && std::fs::read_to_string(checkout.join("Cargo.toml")).is_ok_and(|m| m.contains("name = \"rsmix\"")) {
        println!("cargo:rustc-env=RSMIX_CHECKOUT={}", slashes(&checkout));
    }
}

fn is_template(dir: &Path) -> bool {
    dir.join("src/lib.rs").is_file() && dir.join("src/main.rs").is_file()
}

/// An absolute path with forward slashes, for `include_str!` and the generated Cargo.toml.
fn slashes(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path.display().to_string().replace('\\', "/");
    // canonicalize on Windows gives a verbatim `\\?\` path.
    text.strip_prefix("//?/").map(str::to_string).unwrap_or(text)
}
