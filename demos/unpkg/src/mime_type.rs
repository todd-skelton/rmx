//! `utils/mime-type.ts`: display types, inline image previews and text detection.

const TEXT_EXTENSIONS: &[&str] = &[
    ".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".tsx", ".jsx", ".json", ".md", ".markdown", ".txt", ".css", ".scss", ".less", ".html", ".htm", ".xml", ".svg",
    ".yaml", ".yml", ".toml", ".ini", ".conf", ".sh", ".bash", ".zsh", ".fish", ".ps1", ".bat", ".cmd", ".py", ".rb", ".php", ".java", ".c", ".cpp", ".h", ".hpp",
    ".cs", ".go", ".rs", ".swift", ".kt", ".scala", ".clj", ".ex", ".exs", ".erl", ".hrl", ".lua", ".r", ".sql", ".graphql", ".gql", ".vue", ".svelte", ".astro",
    ".prisma", ".env", ".gitignore", ".npmignore", ".eslintrc", ".prettierrc", ".editorconfig", "LICENSE", "README", "CHANGELOG", "Makefile", "Dockerfile",
];

/// The type shown in directory listings (Remix's `detectMimeType`, or
/// `application/octet-stream` when unknown).
pub fn detect_file_mime_type(filename: &str) -> &'static str {
    rsmix::mime::detect_mime_type(filename).unwrap_or("application/octet-stream")
}

/// The type for an inline `<img>` preview, for the formats browsers display.
pub fn image_mime_type(filename: &str) -> Option<&'static str> {
    Some(match extension(filename).as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".ico" => "image/x-icon",
        ".bmp" => "image/bmp",
        _ => return None,
    })
}

pub fn is_text_content(filename: &str, data: &[u8]) -> bool {
    is_text_file(filename) || is_likely_text(data)
}

/// `.ext` lowercased, or the whole name uppercased when there is no dot (so `README` matches).
pub fn extension(filename: &str) -> String {
    match filename.rfind('.') {
        Some(i) => filename[i..].to_lowercase(),
        None => filename.to_uppercase(),
    }
}

pub fn is_text_file(filename: &str) -> bool {
    TEXT_EXTENSIONS.contains(&extension(filename).as_str())
}

/// No NULs or control characters other than tab, newline and carriage return in the first 8 KiB.
pub fn is_likely_text(data: &[u8]) -> bool {
    data.iter().take(8192).all(|b| *b >= 32 || matches!(b, 9 | 10 | 13))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_type_helpers() {
        assert_eq!(detect_file_mime_type("package.json"), "application/json");
        assert_eq!(detect_file_mime_type("unknown-file.nope"), "application/octet-stream");
        assert_eq!(extension("index.ts"), ".ts");
        assert_eq!(extension("archive.tar.gz"), ".gz");
        assert_eq!(extension(".env"), ".env");
        assert_eq!(extension("README"), "README");
        assert_eq!(image_mime_type("logo.png"), Some("image/png"));
        assert_eq!(image_mime_type("photo.jpg"), Some("image/jpeg"));
        assert_eq!(image_mime_type("photo.jpeg"), Some("image/jpeg"));
        assert_eq!(image_mime_type("README.md"), None);
        assert!(is_text_file("index.ts") && is_text_file("README") && is_text_file(".env"));
        assert!(!is_text_file("logo.png"));
        assert!(is_likely_text(b"hello\nworld"));
        assert!(!is_likely_text(&[104, 0, 105]));
        assert!(!is_likely_text(&[104, 1, 105]));
        assert!(is_text_content("README", &[0]));
        assert!(is_text_content("unknown", b"plain text"));
        assert!(!is_text_content("unknown", &[0]));
    }
}
