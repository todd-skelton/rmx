//! The tools a browser build needs, installed on first use: the `wasm32-unknown-unknown`
//! target, the `wasm-bindgen` CLI at the version rsmix pins, and binaryen's `wasm-opt`.
//!
//! Remix needs none of these (Node runs the TypeScript), so `rsmix dev` and `rsmix build`
//! fetch them the way `npm install` would have: the prebuilt release archives from GitHub,
//! checked against their published SHA-256 and unpacked into a cache that every app shares.
//!
//! Overrides: `RSMIX_WASM_BINDGEN` and `RSMIX_WASM_OPT` name the binaries to use, and
//! `RSMIX_CACHE_DIR` moves the cache. Downloads go through `curl`, which ships with Windows 10
//! and later, macOS and most Linux images. Without it (or on a platform with no release
//! archive) `wasm-bindgen` is built with `cargo install`, and `wasm-opt` is skipped.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rsmix_tar_parser::{TarEntryType, TarEvent, TarOptions, TarParser};

use super::CliError;

/// The `wasm-bindgen` version rsmix builds against (the `=0.2.129` pin in Cargo.toml).
pub const WASM_BINDGEN_VERSION: &str = "0.2.129";
/// The binaryen release `wasm-opt` comes from.
pub const BINARYEN_VERSION: &str = "version_123";
/// The target browser builds compile to.
pub const WASM_TARGET: &str = "wasm32-unknown-unknown";

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// Where downloaded tools live: `RSMIX_CACHE_DIR`, else the platform's cache directory.
pub fn cache_dir() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("RSMIX_CACHE_DIR") {
        return dir;
    }
    let platform = if cfg!(windows) {
        var("LOCALAPPDATA").map(|d| d.join("rsmix").join("cache"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Caches/rsmix"))
    } else {
        var("XDG_CACHE_HOME").map(|d| d.join("rsmix")).or_else(|| var("HOME").map(|h| h.join(".cache/rsmix")))
    };
    platform.unwrap_or_else(|| std::env::temp_dir().join("rsmix-cache"))
}

/// `rustup target list --installed`; `None` when rustup is unavailable.
pub fn installed_targets() -> Option<Vec<String>> {
    let out = Command::new("rustup").args(["target", "list", "--installed"]).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).collect())
}

/// The version a tool's `--version` reports (its second word), or `None` when it does not run.
pub fn tool_version(program: &Path) -> Option<String> {
    let out = Command::new(program).arg("--version").stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).split_whitespace().nth(1).map(str::to_string)
}

/// The cached `wasm-bindgen` (downloaded or built), when there is one.
pub fn cached_wasm_bindgen() -> Option<PathBuf> {
    let dir = wasm_bindgen_dir();
    [dir.join(format!("wasm-bindgen{EXE}")), dir.join("cargo").join("bin").join(format!("wasm-bindgen{EXE}"))].into_iter().find(|p| p.is_file())
}

fn wasm_bindgen_dir() -> PathBuf {
    cache_dir().join(format!("wasm-bindgen-{WASM_BINDGEN_VERSION}"))
}

/// Install the wasm32 target with rustup when it is missing. Without rustup the toolchain
/// is left alone and cargo reports a missing target itself.
pub fn ensure_wasm_target(say: &mut dyn FnMut(&str)) -> Result<(), CliError> {
    match installed_targets() {
        Some(targets) if !targets.iter().any(|t| t == WASM_TARGET) => {
            say(&format!("Installing the {WASM_TARGET} target"));
            match Command::new("rustup").args(["target", "add", WASM_TARGET]).status() {
                Ok(status) if status.success() => Ok(()),
                _ => Err(tool_error(
                    "RMX_WASM_TARGET_UNAVAILABLE",
                    "Could not install the wasm32 target",
                    format!("rustup target add {WASM_TARGET} failed."),
                    "Run `rustup target add wasm32-unknown-unknown` and try again.",
                )),
            }
        }
        _ => Ok(()),
    }
}

/// The `wasm-bindgen` to run: `RSMIX_WASM_BINDGEN`, the cached copy, one on `PATH` at the
/// pinned version, or a freshly downloaded (or, failing that, built) one.
pub fn wasm_bindgen(say: &mut dyn FnMut(&str)) -> Result<PathBuf, CliError> {
    if let Some(path) = env_path("RSMIX_WASM_BINDGEN") {
        return Ok(path);
    }
    if let Some(cached) = cached_wasm_bindgen() {
        return Ok(cached);
    }
    let on_path = PathBuf::from("wasm-bindgen");
    if tool_version(&on_path).as_deref() == Some(WASM_BINDGEN_VERSION) {
        return Ok(on_path);
    }
    let dir = wasm_bindgen_dir();
    let downloaded = dir.join(format!("wasm-bindgen{EXE}"));
    if let Some(triple) = wasm_bindgen_triple() {
        let url = format!(
            "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/{WASM_BINDGEN_VERSION}/wasm-bindgen-{WASM_BINDGEN_VERSION}-{triple}.tar.gz"
        );
        say(&format!("Downloading wasm-bindgen {WASM_BINDGEN_VERSION}"));
        let wanted = format!("wasm-bindgen{EXE}");
        match install_archive(&url, &format!("{url}.sha256sum"), &dir, &|name| name == wanted) {
            Ok(()) if downloaded.is_file() => return Ok(downloaded),
            Ok(()) => say("The wasm-bindgen archive has no wasm-bindgen binary; building it instead"),
            Err(e) => say(&format!("Could not download wasm-bindgen ({e}); building it instead")),
        }
    }
    say(&format!("Building wasm-bindgen {WASM_BINDGEN_VERSION} with cargo install (a few minutes, once)"));
    let root = dir.join("cargo");
    let status = Command::new("cargo")
        .args(["install", "wasm-bindgen-cli", "--locked", "--version", &format!("={WASM_BINDGEN_VERSION}"), "--root"])
        .arg(&root)
        .status();
    let built = root.join("bin").join(format!("wasm-bindgen{EXE}"));
    match status {
        Ok(status) if status.success() && built.is_file() => Ok(built),
        _ => Err(tool_error(
            "RMX_WASM_BINDGEN_UNAVAILABLE",
            "Could not install wasm-bindgen",
            format!("Neither the wasm-bindgen {WASM_BINDGEN_VERSION} release nor cargo install worked."),
            "Install it with `cargo install wasm-bindgen-cli --version 0.2.129`, or set RSMIX_WASM_BINDGEN.",
        )),
    }
}

/// The `wasm-opt` to run: `RSMIX_WASM_OPT`, the cached copy, one on `PATH`, or a freshly
/// downloaded one. `None` when none can be had; the client then ships unoptimized.
pub fn wasm_opt(say: &mut dyn FnMut(&str)) -> Option<PathBuf> {
    if let Some(path) = env_path("RSMIX_WASM_OPT") {
        return Some(path);
    }
    let dir = cache_dir().join(format!("binaryen-{BINARYEN_VERSION}"));
    let cached = dir.join("bin").join(format!("wasm-opt{EXE}"));
    if cached.is_file() {
        return Some(cached);
    }
    let on_path = PathBuf::from("wasm-opt");
    if tool_version(&on_path).is_some() {
        return Some(on_path);
    }
    let Some(platform) = binaryen_platform() else {
        say("No wasm-opt release for this platform; the client is not size-optimized (set RSMIX_WASM_OPT)");
        return None;
    };
    let url = format!("https://github.com/WebAssembly/binaryen/releases/download/{BINARYEN_VERSION}/binaryen-{BINARYEN_VERSION}-{platform}.tar.gz");
    say(&format!("Downloading wasm-opt (binaryen {BINARYEN_VERSION})"));
    let wanted = format!("bin/wasm-opt{EXE}");
    match install_archive(&url, &format!("{url}.sha256"), &dir, &|name| name == wanted || name.starts_with("lib/")) {
        Ok(()) if cached.is_file() => Some(cached),
        Ok(()) => {
            say("The binaryen archive has no wasm-opt; the client is not size-optimized");
            None
        }
        Err(e) => {
            say(&format!("Could not download wasm-opt ({e}); the client is not size-optimized"));
            None
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn tool_error(code: &'static str, title: &'static str, message: String, fix: &'static str) -> CliError {
    CliError { code, title, message, fix: Some(fix), show_help: false }
}

/// The wasm-bindgen release archive for this machine (Linux builds are musl, so static).
fn wasm_bindgen_triple() -> Option<&'static str> {
    Some(match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("x86_64", "linux") => "x86_64-unknown-linux-musl",
        ("aarch64", "linux") => "aarch64-unknown-linux-musl",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        _ => return None,
    })
}

/// The binaryen release archive for this machine.
fn binaryen_platform() -> Option<&'static str> {
    Some(match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "windows") => "x86_64-windows",
        ("x86_64", "linux") => "x86_64-linux",
        ("aarch64", "linux") => "aarch64-linux",
        ("x86_64", "macos") => "x86_64-macos",
        ("aarch64", "macos") => "arm64-macos",
        _ => return None,
    })
}

/// `curl -fsSL url`.
fn download(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "--retry", "2", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// Download a `.tar.gz`, check it against the SHA-256 in `sum_url` (`<hex>  <name>`), and
/// unpack the entries `keep` accepts (paths below the archive's top directory) into `dir`.
/// The files land in a scratch directory that is renamed into place, so an interrupted
/// install leaves no half-written tool behind.
fn install_archive(url: &str, sum_url: &str, dir: &Path, keep: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let archive = download(url)?;
    let sums = download(sum_url)?;
    let expected = String::from_utf8_lossy(&sums).split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if expected != rsmix_crypto::hex(&rsmix_crypto::sha256(&archive)) {
        return Err(format!("{url} does not match its published SHA-256"));
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let scratch = dir.with_file_name(format!("{name}.partial-{}", std::process::id()));
    unpack(&archive, &scratch, keep).map_err(|e| format!("unpacking {url}: {e}"))?;
    std::fs::rename(&scratch, dir).map_err(|e| format!("{} -> {}: {e}", scratch.display(), dir.display()))
}

/// Stream a gzipped tar into `out`, keeping regular files (and, on Unix, symlinks such as a
/// library's versioned names) whose path below the top directory `keep` accepts.
fn unpack(archive: &[u8], out: &Path, keep: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let options = TarOptions { max_entry_size: None, max_total_size: None, max_entries: None, ..TarOptions::default() };
    let mut parser = TarParser::new(options);
    let mut gz = flate2::read::GzDecoder::new(archive);
    let mut file: Option<std::fs::File> = None;
    let mut error: Option<String> = None;
    let mut on_event = |event: TarEvent<'_>| {
        if error.is_some() {
            return;
        }
        if let Err(e) = unpack_event(event, out, keep, &mut file) {
            error = Some(e.to_string());
        }
    };
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let n = gz.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        parser.write(&chunk[..n], &mut on_event).map_err(|e| e.to_string())?;
    }
    parser.finish().map_err(|e| e.to_string())?;
    error.map_or(Ok(()), Err)
}

fn unpack_event(event: TarEvent<'_>, out: &Path, keep: &dyn Fn(&str) -> bool, file: &mut Option<std::fs::File>) -> std::io::Result<()> {
    match event {
        TarEvent::Entry(header) => {
            *file = None;
            let Some((_, rel)) = header.name.split_once('/') else { return Ok(()) };
            let rel = rel.trim_end_matches('/');
            if rel.is_empty() || !keep(rel) {
                return Ok(());
            }
            let path = out.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            match header.kind {
                TarEntryType::File | TarEntryType::ContiguousFile => {
                    *file = Some(std::fs::File::create(&path)?);
                    set_mode(&path, header.mode);
                }
                TarEntryType::Symlink => link(header.linkname.as_deref().unwrap_or_default(), &path)?,
                _ => {}
            }
        }
        TarEvent::Data(bytes) => {
            if let Some(f) = file.as_mut() {
                std::io::Write::write_all(f, bytes)?;
            }
        }
        TarEvent::End => *file = None,
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<i64>) {
    use std::os::unix::fs::PermissionsExt;
    let mode = mode.map_or(0o755, |m| (m as u32) & 0o7777) | 0o600;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<i64>) {}

#[cfg(unix)]
fn link(target: &str, path: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(not(unix))]
fn link(_target: &str, _path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gzipped ustar archive with one top directory, like the release archives.
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (name, body) in entries {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(b"0000755");
            header[124..135].copy_from_slice(format!("{:011o}", body.len()).as_bytes());
            header[136..147].copy_from_slice(b"00000000000");
            header[156] = b'0';
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");
            header[148..156].copy_from_slice(b"        ");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
            header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
            tar.extend_from_slice(&header);
            tar.extend_from_slice(body);
            tar.resize(tar.len().div_ceil(512) * 512, 0);
        }
        tar.extend_from_slice(&[0; 1024]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn unpacks_the_wanted_files_below_the_top_directory() {
        let out = std::env::temp_dir().join(format!("rsmix-cli-unpack-{}", std::process::id()));
        let gz = archive(&[
            ("binaryen-version_123/bin/wasm-opt", b"opt"),
            ("binaryen-version_123/bin/wasm-dis", b"dis"),
            ("binaryen-version_123/lib/libbinaryen.a", b"lib"),
        ]);
        unpack(&gz, &out, &|name| name == "bin/wasm-opt" || name.starts_with("lib/")).unwrap();
        assert_eq!(std::fs::read(out.join("bin/wasm-opt")).unwrap(), b"opt");
        assert_eq!(std::fs::read(out.join("lib/libbinaryen.a")).unwrap(), b"lib");
        assert!(!out.join("bin/wasm-dis").exists(), "unwanted files stay out");
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn a_corrupt_archive_is_an_error() {
        let out = std::env::temp_dir().join(format!("rsmix-cli-unpack-bad-{}", std::process::id()));
        assert!(unpack(b"not gzip", &out, &|_| true).is_err());
    }
}
