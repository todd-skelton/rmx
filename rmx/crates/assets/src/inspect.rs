//! Asset inspection (Remix's `assetServer.getAssets()` / `getAssetDetails()`) and the
//! `assets` command built on it (`remix assets`, `remix assets inspect <url-or-file>`).
//!
//! rmx serves every file its access rules allow, so there is no `unsupported` status:
//! Remix only serves scripts, styles and the file extensions it is configured with. Access
//! rules are file globs only (Remix also allows packages), so [`AssetAccess::allowed_by`]
//! is the glob that matched.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{glob_match, is_script, parse_fingerprint, AssetServer, Inner, Mount};

/// How the asset server handles a file (`AssetKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// Served as is, after any transforms.
    File,
    /// A JavaScript module: its relative imports are rewritten to asset URLs.
    Script,
    /// A stylesheet: its `url(...)` and `@import` references are rewritten.
    Style,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::File => "file",
            AssetKind::Script => "script",
            AssetKind::Style => "style",
        }
    }
}

/// Whether a browser can reach an asset (`AssetStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetStatus {
    /// Mapped, present and allowed.
    Reachable,
    /// Allowed by an `allow_files` glob but refused by a `deny_files` glob.
    Denied,
    /// No `allow_files` glob matches.
    NotAllowed,
    /// Mapped, but no file is there.
    Missing,
    /// No mount covers the URL or file.
    Unmapped,
}

impl AssetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetStatus::Reachable => "reachable",
            AssetStatus::Denied => "denied",
            AssetStatus::NotAllowed => "not-allowed",
            AssetStatus::Missing => "missing",
            AssetStatus::Unmapped => "unmapped",
        }
    }
}

/// The access decision for a file and the rules behind it (`AssetAccessDetails`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetAccess {
    pub allowed: bool,
    /// The first `allow_files` glob that matched.
    pub allowed_by: Option<String>,
    /// The first `deny_files` glob that matched, when one refused the file.
    pub denied_by: Option<String>,
}

/// What the asset server does with a URL or file (`AssetDetails`). Everything but
/// `status` is `None` for unmapped input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetDetails {
    pub status: AssetStatus,
    pub access: Option<AssetAccess>,
    /// The mapped file (absolute).
    pub file_path: Option<PathBuf>,
    /// The mount directory that matched, as configured (`"app"`).
    pub file_root: Option<String>,
    pub kind: Option<AssetKind>,
    /// The asset's public URL pathname, without a fingerprint.
    pub url: Option<String>,
    /// The mount's URL root (`"/assets/app"`).
    pub url_root: Option<String>,
}

impl AssetDetails {
    pub(crate) fn unmapped() -> AssetDetails {
        AssetDetails { status: AssetStatus::Unmapped, access: None, file_path: None, file_root: None, kind: None, url: None, url_root: None }
    }
}

impl AssetServer {
    /// What the server does with a public URL (`/assets/app/x.css`, absolute URLs and
    /// fingerprinted URLs too) or a file path (relative to `root_dir`, or absolute).
    pub fn asset_details(&self, input: &str) -> AssetDetails {
        match self.0.resolve_input(input) {
            Some((mount, file)) => self.0.inspect(mount, file),
            None => AssetDetails::unmapped(),
        }
    }

    /// Every file a browser can currently reach, sorted by URL (`getAssets`). Files are
    /// found below the static prefix of each `allow_files` glob.
    pub fn assets(&self) -> Vec<AssetDetails> {
        let mut files = BTreeSet::new();
        for pattern in &self.0.allow {
            collect_files(&discovery_root(&self.0.root, pattern), &mut files);
        }
        let mut assets: Vec<AssetDetails> = files
            .into_iter()
            .filter_map(|file| self.0.mount_for_file(&file).map(|mount| self.0.inspect(mount, file)))
            .filter(|details| details.status == AssetStatus::Reachable)
            .collect();
        assets.sort_by(|a, b| a.url.cmp(&b.url).then_with(|| a.file_path.cmp(&b.file_path)));
        assets
    }
}

impl Inner {
    fn resolve_input(&self, input: &str) -> Option<(&Mount, PathBuf)> {
        if let Some(rest) = input.strip_prefix("file://") {
            let path = rmx_url::decode(rest);
            // `file:///C:/x` on Windows.
            let path = if path.len() > 2 && path.as_bytes()[2] == b':' { &path[1..] } else { &path[..] };
            return self.mapped_file(Path::new(path));
        }
        let is_url = input.split_once("://").is_some_and(|(scheme, _)| {
            scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        });
        if is_url {
            let url = rmx_url::Url::parse(input).ok()?;
            return self.mapped_url(&parse_fingerprint(url.pathname()).0);
        }
        // A file path when it does not start with `/`, or when it is an absolute path to an
        // existing file; otherwise a URL pathname.
        let file = Path::new(input);
        if !input.starts_with('/') || file.is_absolute() {
            let path = if file.is_absolute() { file.to_path_buf() } else { self.root.join(file) };
            if path.exists() || !input.starts_with('/') {
                return self.mapped_file(&path);
            }
        }
        let url = rmx_url::Url::parse(&format!("http://remix.run{input}")).ok()?;
        self.mapped_url(&parse_fingerprint(url.pathname()).0)
    }

    fn mapped_file(&self, file: &Path) -> Option<(&Mount, PathBuf)> {
        let file = file.canonicalize().unwrap_or_else(|_| normalize(file));
        self.mount_for_file(&file).map(|mount| (mount, file))
    }

    fn mount_for_file(&self, file: &Path) -> Option<&Mount> {
        self.mounts.iter().find(|m| file.starts_with(&m.dir) && file != m.dir)
    }

    fn inspect(&self, mount: &Mount, file: PathBuf) -> AssetDetails {
        let exists = file.is_file();
        let file = if exists { file.canonicalize().unwrap_or(file) } else { file };
        let rel = self.relative(&file).unwrap_or_default();
        let allowed_by = self.allow.iter().find(|g| glob_match(g, &rel)).cloned();
        let denied_by = allowed_by.as_ref().and_then(|_| self.deny.iter().find(|g| glob_match(g, &rel)).cloned());
        let access = AssetAccess { allowed: allowed_by.is_some() && denied_by.is_none(), allowed_by, denied_by };
        let extension = file.extension().map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase())).unwrap_or_default();
        let kind = if is_script(&extension) {
            AssetKind::Script
        } else if extension == ".css" {
            AssetKind::Style
        } else {
            AssetKind::File
        };
        let status = if !exists {
            AssetStatus::Missing
        } else if access.denied_by.is_some() {
            AssetStatus::Denied
        } else if !access.allowed {
            AssetStatus::NotAllowed
        } else {
            AssetStatus::Reachable
        };
        AssetDetails {
            status,
            url: self.url_for_file(&file),
            access: Some(access),
            file_path: Some(file),
            file_root: Some(mount.file_root.clone()),
            kind: Some(kind),
            url_root: Some(mount.url_root.clone()),
        }
    }
}

/// `a/./b/../c` becomes `a/c`, for paths that do not exist (so cannot be canonicalized).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The directory to search for a glob's files: its static prefix (`resolveDiscoveryRoot`).
fn discovery_root(root: &Path, pattern: &str) -> PathBuf {
    let Some(dynamic) = pattern.find(|c| "*?[]{}()!+@".contains(c)) else { return root.join(pattern) };
    let raw = &pattern[..dynamic];
    let prefix = raw.trim_end_matches(['/', '\\']);
    if prefix.is_empty() {
        return root.to_path_buf();
    }
    if raw.ends_with(['/', '\\']) {
        root.join(prefix)
    } else {
        root.join(prefix).parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf)
    }
}

fn collect_files(path: &Path, files: &mut BTreeSet<PathBuf>) {
    let Ok(meta) = std::fs::metadata(path) else { return };
    if meta.is_file() {
        if let Ok(path) = path.canonicalize() {
            files.insert(path);
        }
    } else if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            collect_files(&entry.path(), files);
        }
    }
}

const ASSETS_HELP: &str = "\
Usage:
  assets
  assets inspect <url-or-file>

List or inspect browser-reachable assets.

Commands:
  inspect <url-or-file>  Inspect one asset URL or file

Examples:
  assets
  assets inspect /assets/app/actions/public/entry.js
  assets inspect app/actions/public/entry.js
";

/// Remix's `assets` command for an app binary: when the first argument is `assets`, list
/// the reachable assets (`<url> -> <file>`) or `inspect` one URL or file, then exit.
/// Otherwise return so the app starts.
pub fn assets_command(server: &AssetServer) {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("assets") {
        return;
    }
    let args: Vec<String> = args.collect();
    match assets_command_output(server, &args) {
        Ok(out) => {
            print!("{out}");
            std::process::exit(0);
        }
        Err(e) => {
            eprint!("Error: {e}\n\n{ASSETS_HELP}");
            std::process::exit(1);
        }
    }
}

/// The output of `assets <args>`, or the usage error.
pub fn assets_command_output(server: &AssetServer, args: &[String]) -> Result<String, String> {
    let mut positionals = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(ASSETS_HELP.to_string()),
            other if other.starts_with('-') => return Err(format!("Unknown option {other}.")),
            other => positionals.push(other),
        }
    }
    let root = &server.0.root;
    match positionals.as_slice() {
        [] => {
            let assets = server.assets();
            if assets.is_empty() {
                return Ok("No assets.\n".into());
            }
            Ok(assets
                .iter()
                .map(|a| format!("{} -> {}\n", a.url.as_deref().unwrap_or_default(), display_path(a.file_path.as_deref(), root)))
                .collect())
        }
        ["inspect"] => Err("`assets inspect` requires a URL or file path.".into()),
        ["inspect", input] => {
            let details = server.asset_details(input);
            let mut out = format!("Status: {}\n", details.status.as_str());
            if let Some(url) = &details.url {
                out.push_str(&format!("URL: {url}\n"));
            }
            if details.file_path.is_some() {
                out.push_str(&format!("File: {}\n", display_path(details.file_path.as_deref(), root)));
            }
            if let Some(glob) = details.access.as_ref().and_then(|a| a.denied_by.as_ref()) {
                out.push_str(&format!("Denied by: {glob}\n"));
            }
            Ok(out)
        }
        ["inspect", _, extra, ..] => Err(format!("Unexpected argument {extra}.")),
        [command, ..] => Err(format!("Unknown command: assets {command}")),
    }
}

/// A file relative to the root with `/` separators, or the absolute path outside it.
fn display_path(file: Option<&Path>, root: &Path) -> String {
    let Some(file) = file else { return String::new() };
    match file.strip_prefix(root) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        _ => file.display().to_string(),
    }
}
