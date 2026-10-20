//! File extension to MIME type lookup (Remix's `mime`).

/// The MIME type for a path or file name, from its extension.
pub fn from_path(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
    from_extension(ext)
}

/// The MIME type for an extension (without the dot), or `application/octet-stream`.
pub fn from_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "txt" | "text" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// Whether responses of this type benefit from compression.
pub fn is_compressible(content_type: &str) -> bool {
    let essence = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    essence.starts_with("text/")
        || matches!(
            essence.as_str(),
            "application/json" | "application/javascript" | "application/xml" | "image/svg+xml" | "application/wasm" | "application/manifest+json"
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn looks_up_types() {
        assert_eq!(super::from_path("a/b/app.CSS"), "text/css; charset=utf-8");
        assert_eq!(super::from_path("noext"), "application/octet-stream");
        assert!(super::is_compressible("text/html; charset=utf-8"));
        assert!(!super::is_compressible("image/png"));
    }
}
