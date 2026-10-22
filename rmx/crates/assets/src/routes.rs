//! Mount compilation (Remix's assets `routes.ts`): each mount maps a public URL root below
//! the base path onto a directory, and URL pathnames and file paths convert both ways.
//! File paths use the internal form of [`crate::paths`].

use crate::paths::{is_absolute_file_path, normalize_file_path, normalize_pathname, real_path, resolve_file_path};

/// One set of mounts and the directory their values are relative to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountConfig {
    /// `(url root, directory)` pairs.
    pub mounts: Vec<(String, String)>,
    pub root_dir: String,
}

/// A mount after compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledMount {
    /// The directory in internal form, its real path when it exists.
    pub file_root: String,
    /// The directory as configured.
    pub file_root_value: String,
    /// The canonical URL root, base path included (`/assets/app`).
    pub url_root: String,
    /// The URL root as configured.
    pub url_root_key: String,
}

/// What a URL pathname or file path matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRouteMatch {
    pub file_path: String,
    /// The mount's directory as configured.
    pub file_root: String,
    pub url_pathname: String,
    pub url_root: String,
}

/// Compiled mounts (Remix's `CompiledRoutes`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledRoutes {
    pub mounts: Vec<CompiledMount>,
}

/// Compile mount configs below `base_path`, rejecting absolute directories, URL roots that
/// are not plain pathnames, and overlapping roots, with Remix's messages.
pub fn compile_routes(base_path: &str, configs: &[MountConfig]) -> Result<CompiledRoutes, String> {
    let base = normalize_mount_url_root(base_path)?;
    let mut mounts = Vec::new();
    for config in configs {
        let mut compiled = Vec::new();
        for (url_root, file_root) in &config.mounts {
            compiled.push(compile_mount(&base, url_root, file_root, &config.root_dir)?);
        }
        for (i, a) in compiled.iter().enumerate() {
            for b in &compiled[i + 1..] {
                if roots_overlap(&a.file_root, &b.file_root) {
                    return Err(format!(
                        "mounts values must not overlap. Received \"{}\" and \"{}\", resolving to \"{}\" and \"{}\".",
                        a.file_root_value, b.file_root_value, a.file_root, b.file_root
                    ));
                }
            }
        }
        mounts.extend(compiled);
    }
    for (i, a) in mounts.iter().enumerate() {
        for b in &mounts[i + 1..] {
            if roots_overlap(&a.url_root, &b.url_root) {
                return Err(format!("mounts keys must not overlap. Received \"{}\" and \"{}\".", a.url_root_key, b.url_root_key));
            }
        }
    }
    Ok(CompiledRoutes { mounts })
}

impl CompiledRoutes {
    /// The file a URL pathname maps to, or `None` outside every mount (including encoded
    /// paths that escape their mount).
    pub fn resolve_url_pathname(&self, pathname: &str) -> Option<String> {
        self.match_url_pathname(pathname).map(|m| m.file_path)
    }

    pub fn match_url_pathname(&self, pathname: &str) -> Option<AssetRouteMatch> {
        let pathname = normalize_pathname(pathname);
        for mount in &self.mounts {
            let Some(relative) = path_within_root(&mount.url_root, &pathname) else { continue };
            let file_path = resolve_file_path(&mount.file_root, &rmx_url::decode(relative));
            path_within_root(&mount.file_root, &file_path)?;
            return Some(AssetRouteMatch { file_path, file_root: mount.file_root_value.clone(), url_pathname: pathname, url_root: mount.url_root.clone() });
        }
        None
    }

    /// The URL pathname for a file, each segment percent-encoded like `encodeURIComponent`.
    pub fn to_url_pathname(&self, file_path: &str) -> Option<String> {
        self.match_file_path(file_path).map(|m| m.url_pathname)
    }

    pub fn match_file_path(&self, file_path: &str) -> Option<AssetRouteMatch> {
        let file_path = normalize_file_path(file_path);
        self.mounts.iter().find_map(|mount| {
            let relative = path_within_root(&mount.file_root, &file_path)?;
            let encoded: Vec<String> = relative.split('/').map(rmx_url::encode_component).collect();
            Some(AssetRouteMatch {
                url_pathname: join_url_path(&mount.url_root, &encoded.join("/")),
                file_path: file_path.clone(),
                file_root: mount.file_root_value.clone(),
                url_root: mount.url_root.clone(),
            })
        })
    }
}

fn compile_mount(base: &str, url_root: &str, file_root: &str, root_dir: &str) -> Result<CompiledMount, String> {
    if is_absolute_file_path(file_root) {
        return Err(format!("mounts values must be relative to rootDir. Received \"{file_root}\"."));
    }
    Ok(CompiledMount {
        file_root: resolve_mount_file_root(root_dir, file_root),
        file_root_value: file_root.to_string(),
        url_root: join_url_path(base, &normalize_mount_url_root(url_root)?),
        url_root_key: url_root.to_string(),
    })
}

/// A mount's directory: resolved from `root_dir`, through symlinks when it exists.
pub fn resolve_mount_file_root(root_dir: &str, file_root: &str) -> String {
    let resolved = resolve_file_path(root_dir, file_root);
    let resolved = real_path(&resolved).unwrap_or(resolved);
    let trimmed = resolved.trim_end_matches('/');
    if trimmed.is_empty() { "/".into() } else { trimmed.to_string() }
}

/// A mount key as the pathname a URL parser would produce (non-ASCII and spaces
/// percent-encoded), without a trailing slash. Query strings, fragments and encoded dot
/// segments are rejected.
fn normalize_mount_url_root(url_root: &str) -> Result<String, String> {
    let normalized = normalize_pathname(url_root);
    let normalized = normalized.trim_end_matches('/');
    let normalized = if normalized.is_empty() { "/" } else { normalized };
    let dot_segment = |s: &str| {
        let s = s.to_ascii_lowercase().replace("%2e", ".");
        s == "." || s == ".."
    };
    if normalized.contains(['?', '#']) || normalized.split('/').any(dot_segment) {
        return Err(format!(
            "mounts keys must be URL pathnames without query strings, fragments, or encoded dot segments. Received \"{url_root}\"."
        ));
    }
    let encoded = rmx_url::encode_path(normalized);
    let trimmed = encoded.trim_end_matches('/');
    Ok(if trimmed.is_empty() { "/".into() } else { trimmed.to_string() })
}

fn join_url_path(root: &str, path: &str) -> String {
    if path.is_empty() || path == "/" {
        return root.to_string();
    }
    normalize_pathname(&format!("{}/{}", root.trim_end_matches('/'), path.trim_start_matches('/')))
}

fn path_within_root<'a>(root: &str, value: &'a str) -> Option<&'a str> {
    if value == root {
        return Some("");
    }
    if root == "/" {
        return Some(&value[1..]);
    }
    value.strip_prefix(root)?.strip_prefix('/')
}

fn roots_overlap(a: &str, b: &str) -> bool {
    a == b || a == "/" || b == "/" || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

#[cfg(test)]
mod tests {
    //! Remix's routes.test.ts, one test per case. The roots do not exist on disk, so file
    //! roots are used as written.

    use super::*;

    fn routes(mounts: &[(&str, &str)], root_dir: &str) -> Result<CompiledRoutes, String> {
        let mounts = mounts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        compile_routes("/assets", &[MountConfig { mounts, root_dir: root_dir.into() }])
    }

    fn project(mounts: &[(&str, &str)]) -> CompiledRoutes {
        routes(mounts, "/repo/project").unwrap()
    }

    fn error(mounts: &[(&str, &str)]) -> String {
        routes(mounts, "/repo/project").unwrap_err()
    }

    #[test]
    fn supports_mount_paths_without_leading_slashes() {
        let r = project(&[("app", "app")]);
        assert_eq!(r.resolve_url_pathname("/assets/app/entry.ts").as_deref(), Some("/repo/project/app/entry.ts"));
        assert_eq!(r.to_url_pathname("/repo/project/app/entry.ts").as_deref(), Some("/assets/app/entry.ts"));
    }

    #[test]
    fn supports_mount_paths_with_leading_slashes() {
        let r = project(&[("/app", "app")]);
        assert_eq!(r.resolve_url_pathname("/assets/app/entry.ts").as_deref(), Some("/repo/project/app/entry.ts"));
        assert_eq!(r.to_url_pathname("/repo/project/app/entry.ts").as_deref(), Some("/assets/app/entry.ts"));
    }

    #[test]
    fn normalizes_leading_and_trailing_slashes_in_mount_paths() {
        let r = project(&[("/app/", "app")]);
        assert_eq!(r.resolve_url_pathname("/assets/app/entry.ts").as_deref(), Some("/repo/project/app/entry.ts"));
        assert_eq!(r.to_url_pathname("/repo/project/app/entry.ts").as_deref(), Some("/assets/app/entry.ts"));
    }

    #[test]
    fn canonicalizes_mount_url_roots_as_url_pathnames() {
        let r = project(&[("caf\u{e9} files", "app")]);
        assert_eq!(r.resolve_url_pathname("/assets/caf%C3%A9%20files/entry.ts").as_deref(), Some("/repo/project/app/entry.ts"));
        assert_eq!(r.to_url_pathname("/repo/project/app/entry.ts").as_deref(), Some("/assets/caf%C3%A9%20files/entry.ts"));
    }

    #[test]
    fn supports_windows_style_roots() {
        let r = routes(&[("app", "app")], r"C:\Users\runner\project").unwrap();
        assert_eq!(r.resolve_url_pathname("/assets/app/entry.ts").as_deref(), Some("C:/Users/runner/project/app/entry.ts"));
        assert_eq!(r.to_url_pathname(r"C:\Users\runner\project\app\entry.ts").as_deref(), Some("/assets/app/entry.ts"));
    }

    #[test]
    fn supports_unc_roots_when_mapping_file_paths_back_to_urls() {
        let r = routes(&[("app", "app")], r"\\server\share\project").unwrap();
        assert_eq!(r.resolve_url_pathname("/assets/app/entry.ts").as_deref(), Some("//server/share/project/app/entry.ts"));
        assert_eq!(r.to_url_pathname(r"\\server\share\project\app\entry.ts").as_deref(), Some("/assets/app/entry.ts"));
    }

    #[test]
    fn supports_file_roots_outside_the_root_directory() {
        let r = project(&[("packages", "../packages")]);
        assert_eq!(r.resolve_url_pathname("/assets/packages/shared/value.ts").as_deref(), Some("/repo/packages/shared/value.ts"));
        assert_eq!(r.to_url_pathname("/repo/packages/shared/value.ts").as_deref(), Some("/assets/packages/shared/value.ts"));
    }

    #[test]
    fn supports_mount_configs_with_different_root_directories() {
        let config = |k: &str, v: &str, root: &str| MountConfig { mounts: vec![(k.into(), v.into())], root_dir: root.into() };
        let r = compile_routes("/assets", &[config("app", "app", r"C:\repo\project"), config("runtime", "@oxc-project/runtime", r"D:\repo\node_modules")]).unwrap();
        assert_eq!(r.resolve_url_pathname("/assets/runtime/helpers/decorate.js").as_deref(), Some("D:/repo/node_modules/@oxc-project/runtime/helpers/decorate.js"));
        assert_eq!(r.to_url_pathname(r"D:\repo\node_modules\@oxc-project\runtime\helpers\decorate.js").as_deref(), Some("/assets/runtime/helpers/decorate.js"));
    }

    #[test]
    fn decodes_generated_file_paths_while_keeping_url_pathnames_encoded() {
        let r = project(&[("npm", "node_modules")]);
        assert_eq!(
            r.resolve_url_pathname("/assets/npm/%40remix-run/component/jsx-runtime.ts").as_deref(),
            Some("/repo/project/node_modules/@remix-run/component/jsx-runtime.ts")
        );
        assert_eq!(
            r.to_url_pathname("/repo/project/node_modules/@remix-run/component/jsx-runtime.ts").as_deref(),
            Some("/assets/npm/%40remix-run/component/jsx-runtime.ts")
        );
    }

    #[test]
    fn does_not_resolve_encoded_paths_outside_a_mount() {
        let r = project(&[("app", "app")]);
        assert_eq!(r.resolve_url_pathname("/assets/app/%2E%2E/secret.ts"), None);
        assert_eq!(r.resolve_url_pathname("/assets/app/%2Fetc/passwd"), None);
    }

    #[test]
    fn rejects_incompatible_overlapping_url_roots() {
        assert_eq!(error(&[("app", "app"), ("app/routes", "routes")]), "mounts keys must not overlap. Received \"app\" and \"app/routes\".");
    }

    #[test]
    fn rejects_compatible_but_redundant_overlapping_roots() {
        assert!(error(&[("app", "app"), ("app/vendor", "app/vendor")]).starts_with("mounts values must not overlap. Received \"app\" and \"app/vendor\""));
    }

    #[test]
    fn rejects_url_roots_that_overlap_after_url_canonicalization() {
        assert_eq!(error(&[("app files", "app"), ("app%20files", "other")]), "mounts keys must not overlap. Received \"app files\" and \"app%20files\".");
    }

    #[test]
    fn rejects_url_roots_that_overlap_after_slash_normalization() {
        assert_eq!(error(&[("app", "app"), ("/app/", "other")]), "mounts keys must not overlap. Received \"app\" and \"/app/\".");
    }

    #[test]
    fn rejects_nested_url_roots_after_slash_normalization() {
        assert_eq!(error(&[("/app/", "app"), ("/app/routes/", "routes")]), "mounts keys must not overlap. Received \"/app/\" and \"/app/routes/\".");
    }

    fn bad_key(key: &str) -> String {
        format!("mounts keys must be URL pathnames without query strings, fragments, or encoded dot segments. Received \"{key}\".")
    }

    #[test]
    fn rejects_url_roots_containing_query_strings() {
        assert_eq!(error(&[("app?variant", "app")]), bad_key("app?variant"));
    }

    #[test]
    fn rejects_url_roots_containing_fragments() {
        assert_eq!(error(&[("app#fragment", "app")]), bad_key("app#fragment"));
    }

    #[test]
    fn rejects_url_roots_containing_encoded_dot_segments() {
        assert_eq!(error(&[("app/%2e%2e/other", "app")]), bad_key("app/%2e%2e/other"));
    }

    #[test]
    fn rejects_overlapping_file_roots() {
        assert!(error(&[("app", "app"), ("routes", "app/routes")]).starts_with("mounts values must not overlap. Received \"app\" and \"app/routes\""));
    }

    #[test]
    fn rejects_overlapping_file_roots_after_slash_normalization() {
        assert!(error(&[("app", "app/"), ("routes", "app/routes/")]).starts_with("mounts values must not overlap. Received \"app/\" and \"app/routes/\""));
    }

    // rmx-only

    #[test]
    fn rejects_absolute_mount_directories() {
        assert_eq!(error(&[("app", "/srv/app")]), "mounts values must be relative to rootDir. Received \"/srv/app\".");
    }
}
