//! `utils/npm.ts` and `utils/tarball-cache.ts`: registry metadata, version resolution, and
//! tarballs fetched once, gunzipped, cached in file storage and parsed with rmx-tar-parser.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::io::Read;

use bytes::Bytes;
use nodejs_semver::{Range, Version};
use rmx::file_storage::{File, FileStorage};
use rmx::tar_parser::{parse_tar, TarEntryType, TarOptions};
use serde::Deserialize;

use crate::fetch::Fetch;

pub const NPM_REGISTRY: &str = "https://registry.npmjs.org";

#[derive(Clone, Debug, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    pub versions: BTreeMap<String, PackageVersionMetadata>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackageVersionMetadata {
    pub name: String,
    pub version: String,
    pub dist: Dist,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Dist {
    pub tarball: String,
    #[serde(default)]
    pub shasum: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub kind: FileKind,
}

/// Files and directories by path within the package.
pub type PackageFiles = BTreeMap<String, PackageFile>;

pub struct PackageContents {
    pub metadata: PackageVersionMetadata,
    pub files: PackageFiles,
    contents: HashMap<String, Bytes>,
}

impl PackageContents {
    pub fn file_content(&self, path: &str) -> Option<&Bytes> {
        self.contents.get(path)
    }
}

/// The ways a lookup fails that the browser shows as a 404 page, plus everything else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NpmError {
    PackageNotFound(String),
    VersionNotFound { package_name: String, version: String },
    InvalidPath(String),
    Other(String),
}

impl fmt::Display for NpmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NpmError::PackageNotFound(name) => write!(f, "Package not found: {name}"),
            NpmError::VersionNotFound { package_name, version } => write!(f, "Version not found: {package_name}@{version}"),
            NpmError::InvalidPath(path) => write!(f, "Invalid package path: {path}"),
            NpmError::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for NpmError {}

/// The registry client. `cache` holds decompressed tarballs by `name@version`.
#[derive(Clone)]
pub struct Npm<C> {
    fetch: Fetch,
    cache: C,
}

impl<C: FileStorage> Npm<C> {
    pub fn new(fetch: Fetch, cache: C) -> Npm<C> {
        Npm { fetch, cache }
    }

    /// `fetchPackageMetadata(name)`
    pub async fn metadata(&self, package_name: &str) -> Result<PackageMetadata, NpmError> {
        let url = format!("{NPM_REGISTRY}/{}", rmx::url::encode_component(package_name).replacen("%40", "@", 1));
        let req = http::Request::get(&url).header("accept", "application/json").body(()).map_err(|e| NpmError::Other(e.to_string()))?;
        let res = self.fetch.fetch(req).await.map_err(NpmError::Other)?;
        if res.status() == http::StatusCode::NOT_FOUND {
            return Err(NpmError::PackageNotFound(package_name.to_string()));
        }
        if !res.status().is_success() {
            return Err(NpmError::Other(format!("Failed to fetch package metadata: {}", res.status())));
        }
        serde_json::from_slice(res.body()).map_err(|e| NpmError::Other(format!("Invalid package metadata: {e}")))
    }

    /// `fetchPackageContents(name, version)`, given metadata already fetched.
    pub async fn contents(&self, metadata: &PackageMetadata, version: &str) -> Result<PackageContents, NpmError> {
        let resolved = resolve_version(metadata, version)?;
        let Some(version_metadata) = metadata.versions.get(&resolved) else {
            return Err(NpmError::VersionNotFound { package_name: metadata.name.clone(), version: version.to_string() });
        };
        let tarball = self.tarball(&metadata.name, &resolved, &version_metadata.dist.tarball).await?;
        let (files, contents) = parse_tarball(&tarball).map_err(|e| NpmError::Other(e.to_string()))?;
        Ok(PackageContents { metadata: version_metadata.clone(), files, contents })
    }

    async fn tarball(&self, package_name: &str, version: &str, url: &str) -> Result<Bytes, NpmError> {
        let key = tarball_cache_key(package_name, version);
        if let Ok(Some(cached)) = self.cache.get(&key).await {
            return Ok(cached.bytes);
        }
        let req = http::Request::get(url).body(()).map_err(|e| NpmError::Other(e.to_string()))?;
        let res = self.fetch.fetch(req).await.map_err(NpmError::Other)?;
        if !res.status().is_success() {
            return Err(NpmError::Other(format!("Failed to fetch tarball: {}", res.status())));
        }
        let mut decompressed = Vec::new();
        flate2::read::GzDecoder::new(&res.body()[..]).read_to_end(&mut decompressed).map_err(|e| NpmError::Other(format!("Invalid tarball: {e}")))?;
        let decompressed = Bytes::from(decompressed);
        let file = File::new(format!("{package_name}@{version}.tar"), "application/x-tar", decompressed.clone());
        self.cache.set(&key, file).await.map_err(|e| NpmError::Other(e.to_string()))?;
        Ok(decompressed)
    }
}

/// `getTarballCacheKey(name, version)`
pub fn tarball_cache_key(package_name: &str, version: &str) -> String {
    let safe = package_name.replace('/', "-");
    format!("{}@{version}.tgz", safe.strip_prefix('@').unwrap_or(&safe))
}

/// Whether `specifier` is an exact published version (no redirect needed).
pub fn is_fully_resolved_version(metadata: &PackageMetadata, specifier: &str) -> bool {
    metadata.versions.contains_key(specifier)
}

/// A dist-tag, exact version, semver range (`^1.2.0`, `>=1 <2`) or partial version (`18`, `18.2`),
/// resolved to the highest matching published version.
pub fn resolve_version(metadata: &PackageMetadata, specifier: &str) -> Result<String, NpmError> {
    if let Some(version) = metadata.dist_tags.get(specifier) {
        return Ok(version.clone());
    }
    if metadata.versions.contains_key(specifier) {
        return Ok(specifier.to_string());
    }
    let versions: Vec<Version> = metadata.versions.keys().filter_map(|v| Version::parse(v).ok()).collect();
    for range in [specifier.to_string(), format!("{specifier}.x")] {
        if let Ok(range) = Range::parse(&range) {
            if let Some(version) = range.max_satisfying(&versions) {
                return Ok(version.to_string());
            }
        }
    }
    Err(NpmError::VersionNotFound { package_name: metadata.name.clone(), version: specifier.to_string() })
}

/// Files and directories (from the paths) in an npm tarball, minus its `package/` prefix.
pub fn parse_tarball(data: &[u8]) -> Result<(PackageFiles, HashMap<String, Bytes>), rmx::tar_parser::TarError> {
    let mut files = BTreeMap::new();
    let mut contents = HashMap::new();
    let mut directories = BTreeSet::new();
    // Remix keeps tar-parser's 2 MiB entry and 20 MiB archive defaults, which packages like
    // typescript exceed; allow larger packages.
    let options = TarOptions { max_entry_size: Some(64 << 20), max_total_size: Some(512 << 20), max_entries: Some(50_000), ..Default::default() };
    parse_tar(data, &options, |entry| {
        let name = entry.header.name.strip_prefix("package/").unwrap_or(&entry.header.name).to_string();
        if name.is_empty() {
            return;
        }
        let parts: Vec<&str> = name.split('/').collect();
        for i in 1..parts.len() {
            directories.insert(parts[..i].join("/"));
        }
        match entry.header.kind {
            TarEntryType::Directory => {
                directories.insert(name.trim_end_matches('/').to_string());
            }
            TarEntryType::File => {
                let file = PackageFile { name: parts[parts.len() - 1].to_string(), path: name.clone(), size: entry.header.size, kind: FileKind::File };
                files.insert(name.clone(), file);
                contents.insert(name, Bytes::from(entry.body));
            }
            _ => {}
        }
    })?;
    for path in directories {
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        files.insert(path.clone(), PackageFile { name, path, size: 0, kind: FileKind::Directory });
    }
    Ok((files, contents))
}

/// The direct children of `dir`, directories first, then by name.
pub fn files_at_path(files: &PackageFiles, dir: &str) -> Vec<PackageFile> {
    let dir = dir.trim_matches('/');
    let mut out: Vec<PackageFile> = files
        .iter()
        .filter(|(path, _)| match dir {
            "" => !path.contains('/'),
            _ => path.strip_prefix(dir).and_then(|rest| rest.strip_prefix('/')).is_some_and(|rel| !rel.is_empty() && !rel.contains('/')),
        })
        .map(|(_, file)| file.clone())
        .collect();
    // Remix sorts with localeCompare; case-insensitive order with a case tie-break is close.
    out.sort_by(|a, b| {
        (a.kind != FileKind::Directory).cmp(&(b.kind != FileKind::Directory)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// A browser path split into package name, version (default `latest`) and file path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagePath {
    pub name: String,
    pub version: String,
    pub file_path: String,
}

/// `parsePackagePath`: `lodash@4/lib/utils.js`, `@remix-run/cookie@1.0.0/src/index.ts`, `react@^18.2`.
pub fn parse_package_path(path: &str) -> Result<PackagePath, NpmError> {
    let decoded = rmx::url::decode(path);
    let parts: Vec<&str> = decoded.split('/').collect();
    let (mut name, rest) = if path.starts_with('@') {
        if parts.len() < 2 {
            return Err(NpmError::InvalidPath(path.to_string()));
        }
        (format!("{}/{}", parts[0], parts[1]), &parts[2..])
    } else {
        (parts[0].to_string(), &parts[1..])
    };
    let mut version = "latest".to_string();
    if let Some(at) = name.rfind('@').filter(|at| *at > 0) {
        version = name[at + 1..].to_string();
        name.truncate(at);
    }
    Ok(PackagePath { name, version, file_path: rest.join("/") })
}

#[cfg(test)]
mod tests;
