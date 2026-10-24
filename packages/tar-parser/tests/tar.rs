//! Port of Remix's `tar.test.ts`, one test per case.

mod common;

use std::collections::BTreeMap;

use common::*;
use rsmix_tar_parser::*;

fn all(name: &str) -> Vec<TarEntry> {
    all_with(name, &TarOptions::default())
}

fn all_with(name: &str, options: &TarOptions) -> Vec<TarEntry> {
    entries(&[fixture(name)], options)
}

fn headers_and_text(name: &str) -> Vec<(TarHeader, String)> {
    all(name).into_iter().map(|e| (e.header.clone(), e.text())).collect()
}

fn header(name: &str, size: u64, kind: TarEntryType) -> TarHeader {
    TarHeader {
        name: name.into(),
        mode: Some(0o644),
        uid: Some(501),
        gid: Some(20),
        size,
        mtime: Some(1387580181),
        kind,
        linkname: None,
        uname: "maf".into(),
        gname: "staff".into(),
        devmajor: Some(0),
        devminor: Some(0),
        pax: None,
    }
}

fn pax(pairs: &[(&str, &str)]) -> Option<BTreeMap<String, String>> {
    Some(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

fn gnu_header() -> TarHeader {
    TarHeader {
        uid: Some(12345),
        gid: Some(67890),
        mtime: Some(1559239869),
        uname: "myuser".into(),
        gname: "mygroup".into(),
        ..header("test.txt", 14, TarEntryType::File)
    }
}

// TarParser

#[test]
fn parses_express_4_21_1_tgz() {
    let hashes: BTreeMap<String, String> = all("express-4.21.1.tgz")
        .into_iter()
        .map(|e| (e.header.name, rsmix_crypto::hex(&rsmix_crypto::sha256(&e.body))[..8].to_string()))
        .collect();
    let expected = [
        ("package/LICENSE", "95a57628"),
        ("package/lib/application.js", "5901b32f"),
        ("package/lib/express.js", "2f25585c"),
        ("package/index.js", "4d2f5afc"),
        ("package/lib/router/index.js", "19c5ca9b"),
        ("package/lib/middleware/init.js", "48c1d12f"),
        ("package/lib/router/layer.js", "c90709dc"),
        ("package/lib/middleware/query.js", "6edce396"),
        ("package/lib/request.js", "64ac1075"),
        ("package/lib/response.js", "4b5c338c"),
        ("package/lib/router/route.js", "86db1235"),
        ("package/lib/utils.js", "9035c6d9"),
        ("package/lib/view.js", "ec627880"),
        ("package/package.json", "774eaac2"),
        ("package/History.md", "ca257313"),
        ("package/Readme.md", "016f344e"),
    ];
    assert_eq!(hashes, expected.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
}

#[test]
fn parses_fetch_proxy_0_1_0_tar_gz() {
    assert_eq!(all("fetch-proxy-0.1.0.tar.gz").len(), 192);
}

#[test]
fn parses_lodash_4_17_21_tgz() {
    assert_eq!(all("lodash-4.17.21.tgz").len(), 1054);
}

#[test]
fn parses_npm_11_0_0_tgz() {
    assert_eq!(all("npm-11.0.0.tgz").len(), 2368);
}

// tar-stream test cases

#[test]
fn parses_one_file_tar() {
    assert_eq!(headers_and_text("one-file.tar"), [(header("test.txt", 12, TarEntryType::File), "hello world\n".into())]);
}

#[test]
fn parses_multi_file_tar() {
    assert_eq!(
        headers_and_text("multi-file.tar"),
        [(header("file-1.txt", 12, TarEntryType::File), "i am file-1\n".into()), (header("file-2.txt", 12, TarEntryType::File), "i am file-2\n".into())]
    );
}

#[test]
fn parses_a_file_body_that_ends_at_a_chunk_boundary() {
    let archive = fixture("multi-file.tar");
    let at = 512 + "i am file-1\n".len();
    let entries = entries(&[archive[..at].to_vec(), archive[at..].to_vec()], &TarOptions::default());
    let pairs: Vec<(String, String)> = entries.iter().map(|e| (e.name().to_string(), e.text())).collect();
    assert_eq!(pairs, [("file-1.txt".into(), "i am file-1\n".into()), ("file-2.txt".into(), "i am file-2\n".into())]);
}

#[test]
fn parses_pax_tar() {
    let expected = TarHeader { pax: pax(&[("path", "pax.txt"), ("special", "sauce")]), ..header("pax.txt", 12, TarEntryType::File) };
    assert_eq!(headers_and_text("pax.tar"), [(expected, "hello world\n".into())]);
}

#[test]
fn parses_types_tar() {
    let headers: Vec<TarHeader> = all("types.tar").into_iter().map(|e| e.header).collect();
    let directory = TarHeader { mode: Some(0o755), ..header("directory", 0, TarEntryType::Directory) };
    let link = TarHeader { mode: Some(0o755), linkname: Some("directory".into()), ..header("directory-link", 0, TarEntryType::Symlink) };
    assert_eq!(headers, [directory, link]);
}

#[test]
fn parses_long_name_tar() {
    let name = "my/file/is/longer/than/100/characters/and/should/use/the/prefix/header/foobarbaz/foobarbaz/foobarbaz/foobarbaz/foobarbaz/foobarbaz/filename.txt";
    assert_eq!(headers_and_text("long-name.tar"), [(header(name, 16, TarEntryType::File), "hello long name\n".into())]);
}

#[test]
fn parses_unicode_bsd_tar() {
    let headers: Vec<TarHeader> = all("unicode-bsd.tar").into_iter().map(|e| e.header).collect();
    let expected = TarHeader {
        mtime: Some(1387588646),
        pax: pax(&[
            ("SCHILY.dev", "16777217"),
            ("SCHILY.ino", "3599143"),
            ("SCHILY.nlink", "1"),
            ("atime", "1387589077"),
            ("ctime", "1387588646"),
            ("path", "h\u{f8}ll\u{f8}.txt"),
        ]),
        ..header("h\u{f8}ll\u{f8}.txt", 4, TarEntryType::File)
    };
    assert_eq!(headers, [expected]);
}

#[test]
fn parses_unicode_tar() {
    let headers: Vec<TarHeader> = all("unicode.tar").into_iter().map(|e| e.header).collect();
    let expected = TarHeader { pax: pax(&[("path", "h\u{f8}st\u{e5}l.txt")]), ..header("h\u{f8}st\u{e5}l.txt", 8, TarEntryType::File) };
    assert_eq!(headers, [expected]);
}

#[test]
fn parses_name_is_100_tar() {
    let entries: Vec<(usize, String)> = all("name-is-100.tar").iter().map(|e| (e.name().len(), e.text())).collect();
    assert_eq!(entries, [(100, "hello\n".into())]);
}

#[test]
fn parses_space_tar() {
    assert_eq!(all("space.tar").len(), 4);
}

#[test]
fn parses_gnu_long_path_tar() {
    assert_eq!(all("gnu-long-path.tar").len(), 1);
}

#[test]
fn parses_base_256_uid_gid_tar() {
    let headers: Vec<TarHeader> = all("base-256-uid-gid.tar").into_iter().map(|e| e.header).collect();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].uid, Some(116435139));
    assert_eq!(headers[0].gid, Some(1876110778));
}

#[test]
fn parses_base_256_size_tar() {
    let headers: Vec<TarHeader> = all("base-256-size.tar").into_iter().map(|e| e.header).collect();
    assert_eq!(headers, [header("test.txt", 12, TarEntryType::File)]);
}

#[test]
fn parses_latin1_tar() {
    let options = TarOptions { filename_encoding: FilenameEncoding::Latin1, ..Default::default() };
    let entries: Vec<(TarHeader, String)> = all_with("latin1.tar", &options).into_iter().map(|e| (e.header.clone(), e.text())).collect();
    let expected = TarHeader {
        uid: Some(0),
        gid: Some(0),
        mtime: Some(1495941034),
        uname: "root".into(),
        gname: "root".into(),
        ..header("En fran\u{e7}ais, s'il vous pla\u{ee}t?.txt", 14, TarEntryType::File)
    };
    assert_eq!(entries, [(expected, "Hello, world!\n".into())]);
}

#[test]
fn throws_when_parsing_incomplete_tar() {
    assert_eq!(error(&[fixture("incomplete.tar")], &TarOptions::default()), invalid("Unexpected end of archive"));
}

#[test]
fn parses_gnu_tar() {
    assert_eq!(headers_and_text("gnu.tar"), [(gnu_header(), "Hello, world!\n".into())]);
}

#[test]
fn parses_gnu_incremental_tar() {
    assert_eq!(headers_and_text("gnu-incremental.tar"), [(gnu_header(), "Hello, world!\n".into())]);
}
