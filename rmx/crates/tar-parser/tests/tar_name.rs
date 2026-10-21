//! Port of Remix's `tar-name.test.ts`, one test per case.

mod common;

use common::*;
use rmx_tar_parser::*;

fn options() -> TarOptions {
    TarOptions::default()
}

fn preserve() -> TarOptions {
    TarOptions { path_policy: PathPolicy::Preserve, ..Default::default() }
}

fn parse_header(header: &Header) -> Result<TarHeader, TarError> {
    parse_tar_header(&header.bytes(), &options())
}

/// The archive fails with `message` before any entry reaches the handler.
fn assert_fails(chunks: Vec<Vec<u8>>, message: &str) {
    let (entries, error) = parse_all(&chunks, &options());
    assert_eq!(error, Some(invalid(message)));
    assert!(entries.is_empty(), "{} entries reached the handler", entries.len());
}

fn assert_invalid_name(chunks: Vec<Vec<u8>>) {
    assert_fails(chunks, "Invalid tar entry name");
}

fn assert_invalid_link(chunks: Vec<Vec<u8>>) {
    assert_fails(chunks, "Invalid tar link target");
}

fn names(chunks: Vec<Vec<u8>>, options: &TarOptions) -> Vec<String> {
    entries(&chunks, options).into_iter().map(|e| e.header.name).collect()
}

fn targets(chunks: Vec<Vec<u8>>, options: &TarOptions) -> Vec<Option<String>> {
    entries(&chunks, options).into_iter().map(|e| e.header.linkname).collect()
}

fn one(name: &str) -> Vec<Vec<u8>> {
    vec![header(name).bytes()]
}

fn link(name: &str, kind: char, target: &str) -> Vec<u8> {
    header(name).kind(kind).link(target).bytes()
}

fn gnu(kind: char, value: &str) -> Vec<Vec<u8>> {
    metadata(kind, value.as_bytes())
}

/// Split a metadata entry's body so its last `tail` bytes arrive in their own chunk.
fn split_body(mut chunks: Vec<Vec<u8>>, tail: usize) -> Vec<Vec<u8>> {
    let body = chunks.remove(1);
    let at = body.len() - tail;
    chunks.insert(1, body[at..].to_vec());
    chunks.insert(1, body[..at].to_vec());
    chunks
}

fn some(values: &[&str]) -> Vec<Option<String>> {
    values.iter().map(|v| Some(v.to_string())).collect()
}

// tar entry names

#[test]
fn rejects_absolute_names_in_direct_header_parsing() {
    assert_eq!(parse_header(&header("/outside.txt")), Err(invalid("Invalid tar entry name")));
}

#[test]
fn rejects_parent_components_in_direct_header_parsing() {
    assert_eq!(parse_header(&header("../outside.txt")), Err(invalid("Invalid tar entry name")));
}

#[test]
fn validates_the_combined_ustar_name_in_direct_header_parsing() {
    assert_eq!(parse_header(&header("outside.txt").prefix("../..")), Err(invalid("Invalid tar entry name")));
    assert_eq!(parse_header(&header("file.txt").prefix("src")).unwrap().name, "src/file.txt");
}

#[test]
fn rejects_absolute_names_before_invoking_the_handler() {
    assert_invalid_name(one("/outside.txt"));
}

#[test]
fn rejects_parent_traversal_at_any_depth() {
    for name in ["../outside.txt", "src/../../outside.txt", "src/../file.txt", "src/.."] {
        assert_invalid_name(one(name));
    }
}

#[test]
fn rejects_empty_names() {
    assert_invalid_name(one(""));
}

#[test]
fn rejects_drive_absolute_and_drive_relative_names() {
    for name in ["C:/outside.txt", "c:outside.txt", "././C:/outside.txt", ".//./C:/outside.txt"] {
        assert_invalid_name(one(name));
    }
}

#[test]
fn rejects_backslash_paths_and_unc_names() {
    for name in ["src\\file.txt", "..\\outside.txt", "\\\\server\\share\\file.txt", "//server/share/file.txt"] {
        assert_invalid_name(one(name));
    }
}

#[test]
fn rejects_absolute_ustar_prefixes() {
    assert_invalid_name(vec![header("outside.txt").prefix("/outside").bytes()]);
}

#[test]
fn rejects_parent_components_in_ustar_prefixes() {
    assert_invalid_name(vec![header("outside.txt").prefix("src/../..").bytes()]);
}

#[test]
fn validates_directory_and_link_entry_names() {
    assert_invalid_name(vec![header("../directory/").kind('5').bytes()]);
    assert_invalid_name(vec![link("../link", '2', "file.txt")]);
}

#[test]
fn preserves_nested_names_dot_prefixes_trailing_slashes_unicode_and_newlines() {
    let list = ["file.txt", "./src/\u{e9}.txt", "./", "src/", "src/.../file.txt", "src/..\n"];
    assert_eq!(names(list.iter().map(|n| header(n).bytes()).collect(), &options()), list);
}

#[test]
fn rejects_gnu_long_name_traversal_after_the_override() {
    assert_invalid_name(concat(vec![gnu('L', "../outside.txt\0"), one("file.txt")]));
}

#[test]
fn rejects_embedded_nuls_in_gnu_long_names() {
    assert_invalid_name(concat(vec![gnu('L', "file\0.txt\0"), one("file.txt")]));
}

#[test]
fn rejects_empty_gnu_names_after_decoding_the_terminator() {
    assert_invalid_name(concat(vec![gnu('L', "\0"), one("file.txt")]));
}

#[test]
fn decodes_a_split_gnu_long_name_terminator_and_preserves_following_content() {
    let name = format!("./src/{}.txt", "\u{e9}".repeat(100));
    let mut content = vec![0u8; 512];
    content[..2].copy_from_slice(b"ok");
    let chunks = concat(vec![split_body(gnu('L', &format!("{name}\0")), 1), vec![header("file.txt").size(2).bytes(), content]]);
    let entries = entries(&chunks, &options());
    let pairs: Vec<(String, String)> = entries.iter().map(|e| (e.name().to_string(), e.text())).collect();
    assert_eq!(pairs, [(name, "ok".to_string())]);
}

#[test]
fn preserves_gnu_long_names_without_a_terminator() {
    assert_eq!(names(concat(vec![gnu('L', "./src/file.txt"), one("file.txt")]), &options()), ["./src/file.txt"]);
}

#[test]
fn rejects_local_pax_traversal_after_overriding_a_valid_header_name() {
    assert_invalid_name(concat(vec![pax_path("../outside.txt", 'x'), one("file.txt")]));
}

#[test]
fn rejects_absolute_global_pax_paths_without_a_local_pax_header() {
    assert_invalid_name(concat(vec![pax_path("/outside.txt", 'g'), one("file.txt")]));
}

#[test]
fn rejects_windows_drive_syntax_in_pax_paths() {
    assert_invalid_name(concat(vec![pax_path("C:/outside.txt", 'x'), one("file.txt")]));
}

#[test]
fn rejects_embedded_and_trailing_nuls_in_pax_paths() {
    assert_invalid_name(concat(vec![pax_path("file\0.txt", 'x'), one("file.txt")]));
    assert_invalid_name(concat(vec![pax_path("file.txt\0", 'x'), one("file.txt")]));
}

#[test]
fn validates_the_final_override_without_rejecting_superseded_metadata() {
    let chunks = concat(vec![
        pax_path("../global.txt", 'g'),
        gnu('L', "../gnu.txt\0"),
        pax_path("./src/\u{e9}.txt", 'x'),
        vec![header("../header.txt").prefix("../prefix").bytes()],
    ]);
    assert_eq!(names(chunks, &options()), ["./src/\u{e9}.txt"]);
}

#[test]
fn preserves_pax_deletion_of_a_path_override() {
    let chunks = concat(vec![pax_path("../global.txt", 'g'), pax_path("", 'x'), one("file.txt")]);
    assert_eq!(names(chunks, &options()), ["file.txt"]);
}

// tar link paths

#[test]
fn rejects_absolute_targets_in_direct_header_parsing() {
    assert_eq!(parse_header(&header("link").kind('2').link("/outside")), Err(invalid("Invalid tar link target")));
    assert_eq!(parse_header(&header("link").kind('1').link("/outside")), Err(invalid("Invalid tar link target")));
}

#[test]
fn validates_symlink_targets_relative_to_the_combined_ustar_parent() {
    let parsed = parse_header(&header("current").prefix("lib").kind('2').link("../shared/v2")).unwrap();
    assert_eq!(parsed.linkname.as_deref(), Some("../shared/v2"));
    assert_eq!(parse_header(&header("current").kind('2').link("../shared/v2")), Err(invalid("Invalid tar link target")));
}

#[test]
fn rejects_absolute_targets_before_invoking_the_handler() {
    assert_invalid_link(vec![link("link", '2', "/outside")]);
    assert_invalid_link(vec![link("link", '1', "/outside")]);
}

#[test]
fn rejects_missing_link_targets() {
    assert_invalid_link(vec![header("link").kind('2').bytes()]);
    assert_invalid_link(vec![header("link").kind('1').bytes()]);
}

#[test]
fn rejects_windows_drive_prefixes_backslashes_and_unc_targets() {
    assert_invalid_link(vec![link("link", '2', "C:/outside")]);
    assert_invalid_link(vec![link("link", '1', "c:outside")]);
    assert_invalid_link(vec![link("link", '2', ".//./C:/outside")]);
    assert_invalid_link(vec![link("link", '2', "..\\outside")]);
    assert_invalid_link(vec![link("link", '1', "\\\\server\\share")]);
    assert_invalid_link(vec![link("link", '2', "//server/share")]);
}

#[test]
fn rejects_symlink_targets_that_traverse_above_the_archive_root() {
    assert_invalid_link(vec![link("link", '2', "../outside")]);
    assert_invalid_link(vec![link("lib/link", '2', "../../outside")]);
    assert_invalid_link(vec![link("link", '2', "../lib/file")]);
}

#[test]
fn does_not_count_dots_repeated_separators_or_trailing_slashes_as_parent_directories() {
    assert_invalid_link(vec![link("./link", '2', "../outside")]);
    assert_invalid_link(vec![link("lib//link", '2', "../../outside")]);
    assert_invalid_link(vec![link("lib/./link", '2', "../../outside")]);
    assert_invalid_link(vec![link("lib/link/", '2', "../../outside")]);
}

#[test]
fn preserves_symlink_targets_that_stay_within_the_archive() {
    let chunks = vec![link("lib/current", '2', "../shared/v2"), link("./lib//current", '2', "./v1/../\u{e9}\n"), link("lib/current", '2', ".."), link("current", '2', ".")];
    assert_eq!(targets(chunks, &options()), some(&["../shared/v2", "./v1/../\u{e9}\n", "..", "."]));
}

#[test]
fn resolves_hard_link_targets_from_the_archive_root_not_the_link_parent() {
    assert_invalid_link(vec![link("lib/current", '1', "../shared/v2")]);
    assert_eq!(targets(vec![link("lib/current", '1', "./shared/../file.txt")], &options()), some(&["./shared/../file.txt"]));
}

#[test]
fn rejects_gnu_long_link_traversal_after_overriding_a_safe_header_target() {
    assert_invalid_link(concat(vec![gnu('K', "../outside\0"), vec![link("link", '2', "file.txt")]]));
}

#[test]
fn rejects_empty_and_embedded_nul_gnu_link_targets() {
    assert_invalid_link(concat(vec![gnu('K', "\0"), vec![link("link", '2', "file.txt")]]));
    assert_invalid_link(concat(vec![gnu('K', "file\0.txt\0"), vec![link("link", '1', "file.txt")]]));
}

#[test]
fn decodes_a_fragmented_gnu_link_terminator_and_clears_the_override_for_the_next_entry() {
    let target = format!("../shared/{}", "\u{e9}".repeat(100));
    let chunks = concat(vec![split_body(gnu('K', &format!("{target}\0")), 2), vec![header("lib/current").kind('2').bytes(), link("next", '2', "local")]]);
    assert_eq!(targets(chunks, &options()), some(&[&target, "local"]));
}

#[test]
fn preserves_gnu_link_targets_without_a_terminator() {
    let chunks = concat(vec![gnu('K', "./file.txt"), vec![header("link").kind('1').bytes()]]);
    assert_eq!(targets(chunks, &options()), some(&["./file.txt"]));
}

#[test]
fn rejects_local_and_global_pax_link_targets_after_overrides() {
    assert_invalid_link(concat(vec![pax("linkpath", "../outside", 'x'), vec![link("link", '2', "file.txt")]]));
    assert_invalid_link(concat(vec![pax("linkpath", "/outside", 'g'), vec![link("link", '1', "file.txt")]]));
}

#[test]
fn rejects_embedded_and_trailing_nuls_in_pax_link_targets() {
    assert_invalid_link(concat(vec![pax("linkpath", "file\0.txt", 'x'), vec![header("link").kind('2').bytes()]]));
    assert_invalid_link(concat(vec![pax("linkpath", "file.txt\0", 'x'), vec![header("link").kind('1').bytes()]]));
}

#[test]
fn uses_the_final_gnu_and_pax_entry_names_to_locate_the_symlink_parent() {
    let chunks = concat(vec![gnu('L', "lib/current\0"), vec![link("current", '2', "../shared")], pax_path("lib/current", 'x'), vec![link("current", '2', "../shared")]]);
    assert_eq!(targets(chunks, &options()), some(&["../shared", "../shared"]));
    assert_invalid_link(concat(vec![pax_path("current", 'x'), vec![link("lib/current", '2', "../shared")]]));
}

#[test]
fn validates_the_final_link_target_without_rejecting_superseded_metadata() {
    let chunks = concat(vec![pax("linkpath", "/global", 'g'), gnu('K', "../gnu\0"), pax("linkpath", "./file.txt", 'x'), vec![link("link", '2', "/header")]]);
    assert_eq!(targets(chunks, &options()), some(&["./file.txt"]));
}

#[test]
fn preserves_pax_deletion_of_a_link_target_override() {
    let chunks = concat(vec![pax("linkpath", "/global", 'g'), pax("linkpath", "", 'x'), vec![link("link", '2', "file.txt")]]);
    assert_eq!(targets(chunks, &options()), some(&["file.txt"]));
}

// tar path policy

#[test]
fn enforces_an_explicit_relative_policy() {
    let relative = TarOptions { path_policy: PathPolicy::Relative, ..Default::default() };
    let name = header("../file.txt").bytes();
    assert_eq!(parse_tar_header(&name, &relative), Err(invalid("Invalid tar entry name")));
    assert_eq!(parse_tar(&name, &relative, |_| {}), Err(invalid("Invalid tar entry name")));
    let target = link("link", '2', "/outside");
    assert_eq!(parse_tar_header(&target, &relative), Err(invalid("Invalid tar link target")));
    assert_eq!(parse_tar(&target, &relative, |_| {}), Err(invalid("Invalid tar link target")));
}

#[test]
fn preserves_combined_names_in_direct_header_parsing() {
    assert_eq!(parse_tar_header(&header("file.txt").prefix("/etc").bytes(), &preserve()).unwrap().name, "/etc/file.txt");
}

#[test]
fn preserves_entry_names_and_link_targets_together_in_all_parsing_apis() {
    let block = header("link").prefix("/etc").kind('2').link("../outside").bytes();
    let parsed = parse_tar_header(&block, &preserve()).unwrap();
    assert_eq!((parsed.name.as_str(), parsed.linkname.as_deref()), ("/etc/link", Some("../outside")));
    let mut paths = Vec::new();
    parse_tar(&block, &preserve(), |e| paths.push((e.header.name, e.header.linkname))).unwrap();
    TarParser::new(preserve()).parse([&block], |e| paths.push((e.header.name, e.header.linkname))).unwrap();
    let expected = ("/etc/link".to_string(), Some("../outside".to_string()));
    assert_eq!(paths, [expected.clone(), expected]);
}

#[test]
fn preserves_unrestricted_gnu_and_pax_link_targets_while_decoding_gnu_terminators() {
    let chunks = concat(vec![
        gnu('K', "../gnu\0target\0"),
        vec![header("link").kind('2').bytes()],
        gnu('K', "\0"),
        vec![link("link", '2', "fallback")],
        pax("linkpath", "/pax\0target\0", 'g'),
        vec![header("link").kind('1').bytes()],
    ]);
    assert_eq!(targets(chunks, &preserve()), some(&["../gnu\0target", "", "/pax\0target\0"]));
}

#[test]
fn preserves_ordinary_names_without_relative_path_restrictions() {
    let list = ["/etc/file.txt", "../file.txt", "C:/file.txt", "src\\file.txt", ""];
    assert_eq!(names(list.iter().map(|n| header(n).bytes()).collect(), &preserve()), list);
}

#[test]
fn preserves_final_gnu_and_pax_names_when_configured_on_tar_parser() {
    let chunks = concat(vec![
        gnu('L', "../gnu\0name\0"),
        one("file.txt"),
        pax_path("../local\0name\n", 'x'),
        one("file.txt"),
        pax_path("/global", 'g'),
        one("file.txt"),
        one("next.txt"),
    ]);
    let mut names = Vec::new();
    TarParser::new(preserve()).parse(&chunks, |e| names.push(e.header.name)).unwrap();
    assert_eq!(names, ["../gnu\0name", "../local\0name\n", "/global", "/global"]);
}

#[test]
fn still_validates_header_structure_and_sizes_when_preserving_names() {
    let mut block = header("/file.txt").bytes();
    block[0] = 0;
    assert!(matches!(parse_tar_header(&block, &preserve()), Err(TarError::Parse(m)) if m.starts_with("Invalid tar header")));
    let negative = header("/file.txt").size(Size::Text(format!("{:0>11}", "-1"))).bytes();
    assert_eq!(parse_tar_header(&negative, &preserve()), Err(invalid("Invalid tar entry size")));
}

#[test]
fn still_enforces_archive_limits_when_preserving_names() {
    let file = |size: u64| vec![header("/file.txt").size(size).bytes()];
    assert_eq!(error(&file(1), &TarOptions { max_entry_size: Some(0), ..preserve() }), TarError::MaxEntrySizeExceeded(0));
    assert_eq!(error(&file(0), &TarOptions { max_total_size: Some(511), ..preserve() }), TarError::MaxTotalSizeExceeded(511));
    assert_eq!(error(&file(0), &TarOptions { max_entries: Some(0), ..preserve() }), TarError::MaxEntriesExceeded(0));
}
