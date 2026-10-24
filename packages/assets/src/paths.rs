//! File path and URL pathname normalization (Remix's assets `paths.ts`). File paths use one
//! internal form on every platform: forward slashes, an uppercase drive letter
//! (`C:/Users/app`) and UNC roots kept as `//server/share`.

/// Backslashes to slashes and an uppercase drive letter (`c:\x` to `C:/x`).
pub fn normalize_windows_path(path: &str) -> String {
    let mut out = path.replace('\\', "/");
    if has_drive_letter(&out) {
        out[..1].make_ascii_uppercase();
    }
    out
}

fn has_drive_letter(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

/// Node's `path.posix.normalize`: resolve `.` and `..`, collapse repeated slashes, and keep
/// a trailing slash.
pub(crate) fn posix_normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|s| *s != "..") {
                    segments.pop();
                } else if !absolute {
                    segments.push("..");
                }
            }
            s => segments.push(s),
        }
    }
    let mut out = segments.join("/");
    if out.is_empty() && !absolute {
        out.push('.');
    }
    if trailing && !out.is_empty() {
        out.push('/');
    }
    if absolute {
        out.insert(0, '/');
    }
    out
}

/// A URL pathname in canonical form: slashes, no `.` or `..` segments, and a leading slash.
pub fn normalize_pathname(pathname: &str) -> String {
    let normalized = posix_normalize(&normalize_windows_path(pathname));
    if normalized.starts_with('/') {
        normalized
    } else if normalized == "." {
        "/".into()
    } else {
        format!("/{normalized}")
    }
}

/// Absolute on any platform: `/x`, `C:/x` or a UNC path (`C:` alone is not).
pub fn is_absolute_file_path(path: &str) -> bool {
    let normalized = normalize_windows_path(path);
    normalized.starts_with('/') || has_drive_letter(&normalized)
}

fn unc_root(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("//")?;
    let server = rest.find('/').filter(|&i| i > 0)?;
    let share_len = rest[server + 1..].find('/').unwrap_or(rest.len() - server - 1);
    (share_len > 0).then(|| &path[..2 + server + 1 + share_len])
}

/// A file path in the internal form. Relative paths resolve from the current directory.
pub fn normalize_file_path(path: &str) -> String {
    let normalized = normalize_windows_path(path);
    if let Some(root) = unc_root(&normalized) {
        let remainder = &normalized[root.len()..];
        let rest = posix_normalize(if remainder.is_empty() { "/" } else { remainder });
        return if rest == "/" { root.to_string() } else { format!("{root}{rest}") };
    }
    if has_drive_letter(&normalized) || normalized.starts_with('/') {
        return posix_normalize(&normalized);
    }
    let cwd = std::env::current_dir().map(|d| strip_verbatim(&d.to_string_lossy())).unwrap_or_default();
    posix_normalize(&normalize_windows_path(&format!("{cwd}/{normalized}")))
}

/// `path` resolved from `root_dir` unless it is already absolute.
pub fn resolve_file_path(root_dir: &str, path: &str) -> String {
    if is_absolute_file_path(path) {
        return normalize_file_path(path);
    }
    normalize_file_path(&format!("{}/{}", root_dir.trim_end_matches('/'), normalize_windows_path(path)))
}

/// The path from `from` to `to` with `/` separators. Windows paths (drive or UNC) compare
/// case-insensitively, as `path.win32.relative` does; paths on different roots return `to`.
pub fn relative_file_path(from: &str, to: &str) -> String {
    let (from, to) = (normalize_file_path(from), normalize_file_path(to));
    let windows = from.starts_with("//") || to.starts_with("//") || has_drive_letter(&from) || has_drive_letter(&to);
    let root = |p: &str| -> String {
        if let Some(r) = unc_root(p) {
            r.to_string()
        } else if has_drive_letter(p) {
            p[..2].to_string()
        } else {
            String::new()
        }
    };
    let same = |a: &str, b: &str| if windows { a.eq_ignore_ascii_case(b) } else { a == b };
    let (from_root, to_root) = (root(&from), root(&to));
    if !same(&from_root, &to_root) {
        return to;
    }
    let parts = |p: &str, r: &str| -> Vec<String> { p[r.len()..].split('/').filter(|s| !s.is_empty()).map(String::from).collect() };
    let (a, b) = (parts(&from, &from_root), parts(&to, &to_root));
    let common = a.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    let mut out: Vec<String> = std::iter::repeat_n("..".to_string(), a.len() - common).collect();
    out.extend(b[common..].iter().cloned());
    out.join("/")
}

/// A canonicalized Windows path without its `\\?\` verbatim prefix (`\\?\UNC\server`
/// becomes `\\server`), so it compares with paths written the usual way.
pub(crate) fn strip_verbatim(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// The real path of `path` in the internal form, or `None` when it does not exist.
pub(crate) fn real_path(path: &str) -> Option<String> {
    let real = std::fs::canonicalize(path).ok()?;
    Some(normalize_file_path(&strip_verbatim(&real.to_string_lossy())))
}

#[cfg(test)]
mod tests {
    //! Remix's paths.test.ts, one test per case.

    use super::*;

    #[test]
    fn normalizes_windows_separators_and_uppercases_drive_letters() {
        assert_eq!(normalize_windows_path(r"c:\Users\runner\project"), "C:/Users/runner/project");
        assert_eq!(normalize_windows_path(r"\\server\share\file.ts"), "//server/share/file.ts");
        assert_eq!(normalize_windows_path(r"C:\temp/mixed\path.ts"), "C:/temp/mixed/path.ts");
    }

    #[test]
    fn normalizes_url_pathnames_consistently() {
        assert_eq!(normalize_pathname(r"assets\app\entry.ts"), "/assets/app/entry.ts");
        assert_eq!(normalize_pathname("/assets/app/../app/entry.ts"), "/assets/app/entry.ts");
        assert_eq!(normalize_pathname("./assets//app/./entry.ts"), "/assets/app/entry.ts");
    }

    #[test]
    fn detects_absolute_file_paths_across_platforms() {
        assert!(is_absolute_file_path("/Users/runner/project"));
        assert!(is_absolute_file_path(r"C:\Users\runner\project"));
        assert!(is_absolute_file_path(r"\\server\share\project"));
        assert!(!is_absolute_file_path("C:"));
        assert!(!is_absolute_file_path("app/entry.ts"));
    }

    #[test]
    fn normalizes_absolute_file_paths_to_a_standard_internal_form() {
        assert_eq!(normalize_file_path(r"C:\Users\runner\project"), "C:/Users/runner/project");
        assert_eq!(normalize_file_path("/Users/runner/project"), "/Users/runner/project");
        assert_eq!(normalize_file_path(r"c:\Users\runner\..\runner\project"), "C:/Users/runner/project");
        assert_eq!(normalize_file_path(r"C:\temp/mixed\path.ts"), "C:/temp/mixed/path.ts");
    }

    #[test]
    fn preserves_unc_roots_when_normalizing_file_paths() {
        assert_eq!(normalize_file_path(r"\\server\share\folder\..\file.ts"), "//server/share/file.ts");
        assert_eq!(normalize_file_path(r"\\.\c:\temp\folder\..\file.ts"), "//./c:/temp/file.ts");
    }

    #[test]
    fn resolves_relative_file_paths_from_a_normalized_root() {
        assert_eq!(resolve_file_path("C:/Users/runner/project", r"app\entry.ts"), "C:/Users/runner/project/app/entry.ts");
        assert_eq!(resolve_file_path("/Users/runner/project", "app/entry.ts"), "/Users/runner/project/app/entry.ts");
        assert_eq!(resolve_file_path("/Users/runner/project", "./app/../app/entry.ts"), "/Users/runner/project/app/entry.ts");
    }

    #[test]
    fn preserves_already_absolute_file_paths_when_resolving() {
        assert_eq!(resolve_file_path("C:/Users/runner/project", r"D:\work\entry.ts"), "D:/work/entry.ts");
    }

    #[test]
    fn resolves_relative_file_paths_from_unc_roots() {
        assert_eq!(resolve_file_path("//server/share/project", r"app\entry.ts"), "//server/share/project/app/entry.ts");
        assert_eq!(resolve_file_path("//./c:/temp/project", "./app/../app/entry.ts"), "//./c:/temp/project/app/entry.ts");
    }

    #[test]
    fn gets_relative_file_paths_across_normalized_roots() {
        assert_eq!(relative_file_path("/Users/runner/project", "/Users/runner/project/app/entry.ts"), "app/entry.ts");
        assert_eq!(relative_file_path("C:/Users/runner/project", r"C:\Users\runner\project\app\entry.ts"), "app/entry.ts");
        assert_eq!(relative_file_path("//server/share/project", r"\\server\share\project\app\entry.ts"), "app/entry.ts");
    }

    // rsmix-only

    #[test]
    fn relative_paths_climb_out_and_compare_windows_paths_case_insensitively() {
        assert_eq!(relative_file_path("/a/b/c", "/a/d"), "../../d");
        assert_eq!(relative_file_path("C:/Repo/App", "c:/repo/app/x.ts"), "x.ts");
        assert_eq!(relative_file_path("C:/repo", "D:/repo/x.ts"), "D:/repo/x.ts");
    }

    #[test]
    fn strips_verbatim_prefixes() {
        assert_eq!(strip_verbatim(r"\\?\D:\repo"), r"D:\repo");
        assert_eq!(strip_verbatim(r"\\?\UNC\server\share\x"), r"\\server\share\x");
        assert_eq!(normalize_file_path(&strip_verbatim(r"\\?\d:\repo\app")), "D:/repo/app");
    }
}
