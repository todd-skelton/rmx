//! Ported from utils/npm.test.ts.

use super::*;

fn path(name: &str, version: &str, file_path: &str) -> PackagePath {
    PackagePath { name: name.into(), version: version.into(), file_path: file_path.into() }
}

#[test]
fn parses_package_paths() {
    let cases = [
        ("lodash", path("lodash", "latest", "")),
        ("lodash@4.17.21", path("lodash", "4.17.21", "")),
        ("react@18", path("react", "18", "")),
        ("lodash/package.json", path("lodash", "latest", "package.json")),
        ("lodash@4.17.21/package.json", path("lodash", "4.17.21", "package.json")),
        ("lodash@4/lib/utils.js", path("lodash", "4", "lib/utils.js")),
        ("@remix-run/cookie", path("@remix-run/cookie", "latest", "")),
        ("@remix-run/cookie@1.0.0", path("@remix-run/cookie", "1.0.0", "")),
        ("@remix-run/cookie/src/index.ts", path("@remix-run/cookie", "latest", "src/index.ts")),
        ("@remix-run/cookie@1.0.0/src/lib/cookie.ts", path("@remix-run/cookie", "1.0.0", "src/lib/cookie.ts")),
        ("react@%5E18.2", path("react", "^18.2", "")),
        ("lodash@%7E4.17", path("lodash", "~4.17", "")),
        ("react@^18.2", path("react", "^18.2", "")),
    ];
    for (input, expected) in cases {
        assert_eq!(parse_package_path(input), Ok(expected), "{input}");
    }
    assert_eq!(parse_package_path("@remix-run"), Err(NpmError::InvalidPath("@remix-run".into())));
}

fn mock_metadata() -> PackageMetadata {
    let version = |v: &str| {
        let meta = PackageVersionMetadata {
            name: "test-package".into(),
            version: v.into(),
            dist: Dist { tarball: format!("http://example.com/{v}.tgz"), shasum: "abc".into() },
        };
        (v.to_string(), meta)
    };
    PackageMetadata {
        name: "test-package".into(),
        dist_tags: [("latest", "2.0.0"), ("beta", "3.0.0-beta.1"), ("next", "2.1.0-alpha.1")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        versions: ["1.0.0", "1.0.1", "1.1.0", "2.0.0", "3.0.0-beta.1", "2.1.0-alpha.1"].into_iter().map(version).collect(),
    }
}

#[test]
fn resolves_versions() {
    let m = mock_metadata();
    let ok = |spec: &str| resolve_version(&m, spec).unwrap();
    assert_eq!((ok("latest"), ok("beta"), ok("next")), ("2.0.0".into(), "3.0.0-beta.1".into(), "2.1.0-alpha.1".into()));
    assert_eq!((ok("1.0.0"), ok("2.0.0")), ("1.0.0".into(), "2.0.0".into()));
    assert_eq!(ok("1"), "1.1.0");
    assert_eq!(ok("1.0"), "1.0.1");
    assert_eq!(ok("^1.0.0"), "1.1.0");
    assert_eq!(ok("~1.0.0"), "1.0.1");
    assert_eq!(ok(">=1.0.0"), "2.0.0", "prereleases are skipped");
    assert_eq!(ok(">=1.0.0 <2.0.0"), "1.1.0");
    for missing in ["5.0.0", "4", "^5.0.0"] {
        assert_eq!(resolve_version(&m, missing), Err(NpmError::VersionNotFound { package_name: "test-package".into(), version: missing.into() }));
    }
}

#[test]
fn checks_fully_resolved_versions() {
    let m = mock_metadata();
    assert!(is_fully_resolved_version(&m, "1.0.0") && is_fully_resolved_version(&m, "2.0.0"));
    for spec in ["latest", "1", "1.0", "^1.0.0", "~1.0.0", ">=1.0.0", "3.0.0"] {
        assert!(!is_fully_resolved_version(&m, spec), "{spec}");
    }
}

fn mock_files() -> BTreeMap<String, PackageFile> {
    let entry = |path: &str, size: u64, kind: FileKind| {
        let name = path.rsplit('/').next().unwrap().to_string();
        (path.to_string(), PackageFile { name, path: path.into(), size, kind })
    };
    [
        entry("dist", 0, FileKind::Directory),
        entry("src", 0, FileKind::Directory),
        entry("LICENSE", 1024, FileKind::File),
        entry("package.json", 512, FileKind::File),
        entry("README.md", 2048, FileKind::File),
        entry("src/index.ts", 256, FileKind::File),
        entry("src/lib", 0, FileKind::Directory),
        entry("src/lib/utils.ts", 128, FileKind::File),
        entry("dist/index.js", 384, FileKind::File),
        entry("dist/index.d.ts", 192, FileKind::File),
    ]
    .into_iter()
    .collect()
}

fn names(files: &[PackageFile]) -> Vec<&str> {
    files.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn lists_files_at_a_path() {
    let files = mock_files();
    assert_eq!(names(&files_at_path(&files, "")), ["dist", "src", "LICENSE", "package.json", "README.md"]);
    assert_eq!(names(&files_at_path(&files, "src")), ["lib", "index.ts"]);
    assert_eq!(names(&files_at_path(&files, "src/lib")), ["utils.ts"]);
    assert_eq!(names(&files_at_path(&files, "dist")), ["index.d.ts", "index.js"]);
    assert!(files_at_path(&files, "nonexistent").is_empty());
    assert_eq!(files_at_path(&files, "src/").len(), 2);
    assert_eq!(files_at_path(&files, "/src").len(), 2);
    let root = files_at_path(&files, "");
    assert_eq!(root.iter().map(|f| f.kind).collect::<Vec<_>>()[..3], [FileKind::Directory, FileKind::Directory, FileKind::File]);
}

#[test]
fn errors_describe_themselves() {
    assert_eq!(NpmError::PackageNotFound("my-package".into()).to_string(), "Package not found: my-package");
    let version = NpmError::VersionNotFound { package_name: "my-package".into(), version: "5.0.0".into() };
    assert_eq!(version.to_string(), "Version not found: my-package@5.0.0");
    assert_eq!(NpmError::InvalidPath("@incomplete".into()).to_string(), "Invalid package path: @incomplete");
}

#[test]
fn cache_keys_flatten_scopes() {
    assert_eq!(tarball_cache_key("@remix-run/cookie", "1.0.0"), "remix-run-cookie@1.0.0.tgz");
    assert_eq!(tarball_cache_key("lodash", "4.17.21"), "lodash@4.17.21.tgz");
}
