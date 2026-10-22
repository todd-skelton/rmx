//! Ported from remix tar-parser's tar.test.ts, tar-name.test.ts and tar-size.test.ts.

use std::io::Read;

use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    if name.ends_with(".tgz") || name.ends_with(".tar.gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out).unwrap();
        out
    } else {
        bytes
    }
}

fn entries(archive: &[u8], options: &TarOptions) -> Result<Vec<TarEntry>, TarError> {
    let mut out = Vec::new();
    parse_tar(archive, options, |e| out.push(e))?;
    Ok(out)
}

fn all(name: &str) -> Vec<TarEntry> {
    entries(&fixture(name), &TarOptions::default()).unwrap()
}

fn maf(name: &str, size: u64, kind: TarEntryType) -> TarHeader {
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

// --- TarParser -------------------------------------------------------------------------

#[test]
fn parses_express_npm_package() {
    let hashes: BTreeMap<String, String> = all("express-4.21.1.tgz")
        .into_iter()
        .map(|e| (e.header.name, rmx_crypto::hex(&rmx_crypto::sha256(&e.body))[..8].to_string()))
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
fn parses_a_github_archive() {
    assert_eq!(all("fetch-proxy-0.1.0.tar.gz").len(), 192);
}

// --- tar-stream test cases -------------------------------------------------------------

#[test]
fn parses_one_file() {
    let e = all("one-file.tar");
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].header, maf("test.txt", 12, TarEntryType::File));
    assert_eq!(e[0].text(), "hello world\n");
}

#[test]
fn parses_multi_file() {
    let e = all("multi-file.tar");
    assert_eq!(e.iter().map(|e| (e.header.clone(), e.text())).collect::<Vec<_>>(), [
        (maf("file-1.txt", 12, TarEntryType::File), "i am file-1\n".to_string()),
        (maf("file-2.txt", 12, TarEntryType::File), "i am file-2\n".to_string()),
    ]);
}

#[test]
fn parses_bodies_split_at_any_chunk_boundary() {
    let archive = fixture("multi-file.tar");
    let expected = entries(&archive, &TarOptions::default()).unwrap();
    let boundary = 512 + "i am file-1\n".len();
    let mut split = Vec::new();
    parse_tar_chunks([&archive[..boundary], &archive[boundary..]], &TarOptions::default(), |e| split.push(e)).unwrap();
    assert_eq!(split, expected);
    let mut bytewise = Vec::new();
    parse_tar_chunks(archive.chunks(1), &TarOptions::default(), |e| bytewise.push(e)).unwrap();
    assert_eq!(bytewise, expected);

    let pax = fixture("pax.tar");
    let mut bytewise = Vec::new();
    parse_tar_chunks(pax.chunks(7), &TarOptions::default(), |e| bytewise.push(e)).unwrap();
    assert_eq!(bytewise, entries(&pax, &TarOptions::default()).unwrap());
}

#[test]
fn streams_body_chunks_without_buffering() {
    let archive = fixture("multi-file.tar");
    let mut parser = TarParser::default();
    let mut log = Vec::new();
    for chunk in archive.chunks(520) {
        parser
            .write(chunk, &mut |event| match event {
                TarEvent::Entry(h) => log.push(format!("entry {}", h.name)),
                TarEvent::Data(d) => log.push(format!("data {}", d.len())),
                TarEvent::End => log.push("end".into()),
            })
            .unwrap();
    }
    parser.finish().unwrap();
    assert_eq!(log, ["entry file-1.txt", "data 8", "data 4", "end", "entry file-2.txt", "data 12", "end"]);
}

#[test]
fn parses_pax() {
    let e = all("pax.tar");
    let mut header = maf("pax.txt", 12, TarEntryType::File);
    header.pax = pax(&[("path", "pax.txt"), ("special", "sauce")]);
    assert_eq!(e[0].header, header);
    assert_eq!(e[0].text(), "hello world\n");
}

#[test]
fn parses_types() {
    let headers: Vec<_> = all("types.tar").into_iter().map(|e| e.header).collect();
    let mut dir = maf("directory", 0, TarEntryType::Directory);
    dir.mode = Some(0o755);
    let mut link = maf("directory-link", 0, TarEntryType::Symlink);
    link.mode = Some(0o755);
    link.linkname = Some("directory".into());
    assert_eq!(headers, [dir, link]);
}

#[test]
fn parses_long_names_from_the_ustar_prefix() {
    let e = all("long-name.tar");
    let name = "my/file/is/longer/than/100/characters/and/should/use/the/prefix/header/foobarbaz/foobarbaz/foobarbaz/foobarbaz/foobarbaz/foobarbaz/filename.txt";
    assert_eq!(e[0].header, maf(name, 16, TarEntryType::File));
    assert_eq!(e[0].text(), "hello long name\n");
}

#[test]
fn parses_unicode_names() {
    let e = all("unicode-bsd.tar");
    let mut header = maf("høllø.txt", 4, TarEntryType::File);
    header.mtime = Some(1387588646);
    header.pax = pax(&[
        ("SCHILY.dev", "16777217"),
        ("SCHILY.ino", "3599143"),
        ("SCHILY.nlink", "1"),
        ("atime", "1387589077"),
        ("ctime", "1387588646"),
        ("path", "høllø.txt"),
    ]);
    assert_eq!(e[0].header, header);

    let e = all("unicode.tar");
    let mut header = maf("høstål.txt", 8, TarEntryType::File);
    header.pax = pax(&[("path", "høstål.txt")]);
    assert_eq!(e[0].header, header);
}

#[test]
fn parses_odd_fixtures() {
    let e = all("name-is-100.tar");
    assert_eq!((e[0].header.name.len(), e[0].text().as_str()), (100, "hello\n"));
    assert_eq!(all("space.tar").len(), 4);
    assert_eq!(all("gnu-long-path.tar").len(), 1);
    let e = all("base-256-uid-gid.tar");
    assert_eq!((e.len(), e[0].header.uid, e[0].header.gid), (1, Some(116435139), Some(1876110778)));
    assert_eq!(all("base-256-size.tar")[0].header, maf("test.txt", 12, TarEntryType::File));
}

#[test]
fn parses_latin1_names() {
    let options = TarOptions { filename_encoding: FilenameEncoding::Latin1, ..Default::default() };
    let e = entries(&fixture("latin1.tar"), &options).unwrap();
    assert_eq!(e[0].header.name, "En français, s'il vous plaît?.txt");
    assert_eq!((e[0].header.uid, e[0].header.mtime, e[0].header.uname.as_str()), (Some(0), Some(1495941034), "root"));
    assert_eq!(e[0].text(), "Hello, world!\n");
}

#[test]
fn rejects_incomplete_archives() {
    let err = entries(&fixture("incomplete.tar"), &TarOptions::default()).unwrap_err();
    assert_eq!(err, TarError::Parse("Unexpected end of archive".into()));
}

#[test]
fn parses_gnu_archives() {
    for name in ["gnu.tar", "gnu-incremental.tar"] {
        let e = all(name);
        let header = TarHeader {
            uid: Some(12345),
            gid: Some(67890),
            mtime: Some(1559239869),
            uname: "myuser".into(),
            gname: "mygroup".into(),
            ..maf("test.txt", 14, TarEntryType::File)
        };
        assert_eq!(e.len(), 1, "{name}");
        assert_eq!(e[0].header, header, "{name}");
        assert_eq!(e[0].text(), "Hello, world!\n");
    }
}

#[test]
fn rejects_gzipped_input() {
    let gz = std::fs::read(format!("{}/tests/fixtures/express-4.21.1.tgz", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let err = entries(&gz, &TarOptions::default()).unwrap_err();
    assert_eq!(err.to_string(), "Invalid tar header. Maybe the tar is corrupted or needs to be gunzipped?");
}

// --- Constructed archives --------------------------------------------------------------

struct Block {
    name: String,
    kind: u8,
    size_field: Vec<u8>,
    linkname: String,
    prefix: String,
}

fn block(name: &str, kind: u8, size: usize) -> Block {
    Block { name: name.into(), kind, size_field: format!("{size:011o}").into_bytes(), linkname: String::new(), prefix: String::new() }
}

impl Block {
    fn link(mut self, target: &str) -> Block {
        self.linkname = target.into();
        self
    }

    fn prefix(mut self, prefix: &str) -> Block {
        self.prefix = prefix.into();
        self
    }

    fn size_field(mut self, field: &[u8]) -> Block {
        self.size_field = field.to_vec();
        self
    }

    fn bytes(&self) -> [u8; 512] {
        let mut b = [0u8; 512];
        b[..self.name.len()].copy_from_slice(self.name.as_bytes());
        b[100..107].copy_from_slice(b"0000644");
        b[124..124 + self.size_field.len()].copy_from_slice(&self.size_field);
        b[136..147].copy_from_slice(b"00000000000");
        b[156] = self.kind;
        b[157..157 + self.linkname.len()].copy_from_slice(self.linkname.as_bytes());
        b[257..263].copy_from_slice(b"ustar\0");
        b[263..265].copy_from_slice(b"00");
        b[345..345 + self.prefix.len()].copy_from_slice(self.prefix.as_bytes());
        b[148..156].copy_from_slice(b"        ");
        let sum: u32 = b.iter().map(|x| *x as u32).sum();
        b[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        b
    }
}

/// An archive of (header, body) pairs with padding and the end-of-archive marker.
fn archive(parts: &[(Block, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (header, body) in parts {
        out.extend_from_slice(&header.bytes());
        out.extend_from_slice(body);
        out.resize(out.len() + padding(body.len() as u64) as usize, 0);
    }
    out.extend_from_slice(&[0; 1024]);
    out
}

fn pax_records(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in pairs {
        let body = format!(" {k}={v}\n");
        let mut len = body.len() + 1;
        while format!("{len}{body}").len() != len {
            len += 1;
        }
        out.extend_from_slice(format!("{len}{body}").as_bytes());
    }
    out
}

fn names(bytes: &[u8], options: &TarOptions) -> Result<Vec<String>, TarError> {
    entries(bytes, options).map(|e| e.into_iter().map(|e| e.header.name).collect())
}

fn invalid_name() -> TarError {
    TarError::Parse("Invalid tar entry name".into())
}

fn invalid_target() -> TarError {
    TarError::Parse("Invalid tar link target".into())
}

#[test]
fn rejects_unsafe_entry_names() {
    let o = TarOptions::default();
    for bad in ["/etc/passwd", "a/../../b", "..", "C:/x", "c:x", "./C:/x", "a\\b", "\\\\server\\share"] {
        assert_eq!(names(&archive(&[(block(bad, b'0', 1), b"x")]), &o), Err(invalid_name()), "{bad}");
    }
    assert_eq!(names(&archive(&[(block("x", b'0', 0).prefix("/abs"), b"")]), &o), Err(invalid_name()));
    assert_eq!(names(&archive(&[(block("x", b'0', 0).prefix("a/.."), b"")]), &o), Err(invalid_name()));
    let fine = ["a/b/c.txt", "./a", ".hidden", "dir/", "naïve.txt", "line\nbreak"];
    for good in fine {
        assert_eq!(names(&archive(&[(block(good, b'0', 0), b"")]), &o), Ok(vec![good.to_string()]), "{good:?}");
    }
    assert_eq!(parse_tar_header(&block("/abs", b'0', 0).bytes(), &o), Err(invalid_name()));
    let preserve = TarOptions { path_policy: PathPolicy::Preserve, ..Default::default() };
    assert_eq!(names(&archive(&[(block("/etc/../passwd", b'0', 0), b"")]), &preserve), Ok(vec!["/etc/../passwd".to_string()]));
    assert_eq!(parse_tar_header(&block("x", b'0', 0).prefix("/abs").bytes(), &preserve).unwrap().name, "/abs/x");
}

#[test]
fn checks_link_targets_from_the_right_directory() {
    let o = TarOptions::default();
    let link = |name: &str, kind: u8, target: &str| names(&archive(&[(block(name, kind, 0).link(target), b"")]), &o);
    assert_eq!(link("a/b/link", b'2', "../c"), Ok(vec!["a/b/link".to_string()]));
    assert_eq!(link("a/link", b'2', "../c"), Ok(vec!["a/link".to_string()]));
    assert_eq!(link("a/link", b'2', "../../c"), Err(invalid_target()));
    assert_eq!(link("a//./link", b'2', "./x//y/"), Ok(vec!["a//./link".to_string()]));
    assert_eq!(link("a/b/hard", b'1', "../c"), Err(invalid_target()), "hard links resolve from the root");
    assert_eq!(link("a/b/hard", b'1', "a/c"), Ok(vec!["a/b/hard".to_string()]));
    for bad in ["/etc", "C:/x", "a\\b"] {
        assert_eq!(link("l", b'2', bad), Err(invalid_target()), "{bad}");
    }
    assert_eq!(link("l", b'2', ""), Err(invalid_target()), "missing target");
}

#[test]
fn applies_gnu_long_names_and_links() {
    let o = TarOptions::default();
    let long = "d/".repeat(80) + "file.txt";
    let mut data = long.clone().into_bytes();
    data.push(0);
    let bytes = archive(&[(block("././@LongLink", b'L', data.len()), &data), (block("short", b'0', 2), b"hi"), (block("next", b'0', 0), b"")]);
    let e = entries(&bytes, &o).unwrap();
    assert_eq!(e.iter().map(|e| e.header.name.as_str()).collect::<Vec<_>>(), [long.as_str(), "next"]);
    assert_eq!(e[0].text(), "hi");
    let mut split = Vec::new();
    parse_tar_chunks(bytes.chunks(3), &o, |e| split.push(e)).unwrap();
    assert_eq!(split, e, "terminator split across chunks");

    let bytes = archive(&[(block("././@LongLink", b'L', 1), b"\0"), (block("short", b'0', 0), b"")]);
    assert_eq!(names(&bytes, &o), Err(invalid_name()), "empty after the terminator");
    let bytes = archive(&[(block("././@LongLink", b'L', 6), b"../evil"), (block("safe", b'0', 0), b"")]);
    assert_eq!(names(&bytes, &o), Err(invalid_name()));
    let bytes = archive(&[(block("././@LongLink", b'K', 6), b"target"), (block("l", b'2', 0).link("x"), b"")]);
    assert_eq!(entries(&bytes, &o).unwrap()[0].header.linkname.as_deref(), Some("target"));
    let bytes = archive(&[(block("././@LongLink", b'K', 5), b"../up"), (block("l", b'2', 0).link("x"), b"")]);
    assert_eq!(names(&bytes, &o), Err(invalid_target()));
}

#[test]
fn applies_pax_overrides() {
    let o = TarOptions::default();
    let records = pax_records(&[("path", "from-pax.txt"), ("size", "3")]);
    let bytes = archive(&[(block("PaxHeader", b'x', records.len()), &records), (block("header.txt", b'0', 0), b"abc")]);
    let e = entries(&bytes, &o).unwrap();
    assert_eq!((e[0].header.name.as_str(), e[0].header.size, e[0].text().as_str()), ("from-pax.txt", 3, "abc"));

    let records = pax_records(&[("path", "")]);
    let bytes = archive(&[(block("PaxHeader", b'x', records.len()), &records), (block("kept.txt", b'0', 0), b"")]);
    assert_eq!(names(&bytes, &o), Ok(vec!["kept.txt".to_string()]), "an empty path deletes the override");

    let records = pax_records(&[("path", "/abs")]);
    let bytes = archive(&[(block("PaxHeader", b'g', records.len()), &records), (block("a", b'0', 0), b""), (block("b", b'0', 0), b"")]);
    assert_eq!(names(&bytes, &o), Err(invalid_name()), "global PAX paths are checked too");

    let records = pax_records(&[("comment", "everyone")]);
    let bytes = archive(&[(block("g", b'g', records.len()), &records), (block("a", b'0', 0), b""), (block("b", b'0', 0), b"")]);
    let e = entries(&bytes, &o).unwrap();
    assert!(e.iter().all(|e| e.header.pax == pax(&[("comment", "everyone")])));

    for bad in ["x", "-1", "1.5", "12abc", "9007199254740992"] {
        let records = pax_records(&[("size", bad)]);
        let bytes = archive(&[(block("PaxHeader", b'x', records.len()), &records), (block("a", b'0', 0), b"")]);
        assert_eq!(names(&bytes, &o), Err(TarError::Parse("Invalid tar entry size".into())), "{bad}");
    }
}

#[test]
fn validates_size_fields() {
    let o = TarOptions::default();
    let invalid = Err(TarError::Parse("Invalid tar entry size".into()));
    let with_size = |field: &[u8]| parse_tar_header(&block("f", b'0', 0).size_field(field).bytes(), &o).map(|h| h.size);
    assert_eq!(with_size(b"00000000012"), Ok(10));
    assert_eq!(with_size(b"  12  \0"), Ok(10));
    assert_eq!(with_size(b"\0\0\0"), Ok(0));
    assert_eq!(with_size(b"0000000001x"), invalid);
    assert_eq!(with_size(b"00000000089"), invalid);
    let mut base256 = vec![0x80u8; 1];
    base256.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    assert_eq!(with_size(&base256), Ok(256));
    assert_eq!(with_size(&[0xff; 12]), invalid, "negative base-256");
    assert_eq!(with_size(&[0x81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]), invalid, "unsupported marker");
    assert_eq!(with_size(&[0x80, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]), invalid, "beyond the safe integer range");
}

#[test]
fn enforces_limits() {
    let body = vec![b'x'; 2 * 1024 * 1024 + 1];
    let big = archive(&[(block("big", b'0', body.len()), &body)]);
    assert_eq!(names(&big, &TarOptions::default()), Err(TarError::MaxEntrySizeExceeded(2 * 1024 * 1024)));
    let unlimited = TarOptions { max_entry_size: None, ..Default::default() };
    assert_eq!(names(&big, &unlimited).unwrap().len(), 1);

    let small = TarOptions { max_entry_size: Some(4), ..Default::default() };
    assert_eq!(names(&archive(&[(block("f", b'0', 5), b"12345")]), &small), Err(TarError::MaxEntrySizeExceeded(4)));
    let records = pax_records(&[("path", "a-long-pax-path")]);
    let bytes = archive(&[(block("PaxHeader", b'x', records.len()), &records), (block("a", b'0', 0), b"")]);
    assert_eq!(names(&bytes, &small), Err(TarError::MaxEntrySizeExceeded(4)), "metadata entries count");

    let three = archive(&[(block("a", b'0', 0), b""), (block("b", b'0', 0), b""), (block("c", b'0', 0), b"")]);
    let two = TarOptions { max_entries: Some(2), ..Default::default() };
    assert_eq!(names(&three, &two), Err(TarError::MaxEntriesExceeded(2)));
    let total = TarOptions { max_total_size: Some(1024), ..Default::default() };
    assert_eq!(names(&three, &total), Err(TarError::MaxTotalSizeExceeded(1024)));
    assert_eq!(TarError::MaxTotalSizeExceeded(1024).to_string(), "Tar archive size exceeds maximum allowed size of 1024 bytes");

    let preserve = TarOptions { path_policy: PathPolicy::Preserve, max_entries: Some(2), ..Default::default() };
    assert_eq!(names(&three, &preserve), Err(TarError::MaxEntriesExceeded(2)));
}

#[test]
fn checks_header_structure() {
    let o = TarOptions::default();
    assert_eq!(parse_tar_header(&[0; 100], &o), Err(TarError::Parse("Invalid tar header size".into())));
    let mut b = block("f", b'0', 0).bytes();
    b[0] = b'g';
    assert!(matches!(parse_tar_header(&b, &o), Err(TarError::Parse(m)) if m.starts_with("Invalid tar header. Maybe")));
    let mut b = block("f", b'0', 0).bytes();
    b[257..265].copy_from_slice(b"weird\0\0\0");
    b[148..156].copy_from_slice(b"        ");
    let sum: u32 = b.iter().map(|x| *x as u32).sum();
    b[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
    assert!(parse_tar_header(&b, &o).is_ok());
    let strict = TarOptions { allow_unknown_format: false, ..Default::default() };
    assert_eq!(parse_tar_header(&b, &strict), Err(TarError::Parse("Invalid tar header, unknown format".into())));
}
