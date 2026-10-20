//! `utils/format-bytes.ts`

pub fn format_bytes(bytes: u64) -> String {
    match bytes {
        0 => "-".into(),
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.1} kB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
    }
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_like_remix() {
        assert_eq!(format_bytes(0), "-");
        for (n, s) in [(1, "1 B"), (100, "100 B"), (1023, "1023 B"), (1024, "1.0 kB"), (1536, "1.5 kB"), (10240, "10.0 kB"), (1024 * 1023, "1023.0 kB")] {
            assert_eq!(format_bytes(n), s);
        }
        for (n, s) in [(1024 * 1024, "1.0 MB"), (1024 * 1024 * 5 / 2, "2.5 MB"), (1024 * 1024 * 100, "100.0 MB")] {
            assert_eq!(format_bytes(n), s);
        }
    }
}
