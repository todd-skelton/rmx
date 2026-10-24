//! Port of Remix's `tar-size.test.ts`, one test per case.
//!
//! Remix spies on `Uint8Array` construction to check allocations; here a counting global
//! allocator records the current thread's allocations while a parse runs. It counts every
//! allocation (strings and maps too), not only byte buffers, so the linear-buffering bounds
//! below are looser than Remix's `4 * length` while still ruling out quadratic growth.
//!
//! Remix's handlers receive an entry whose body is a stream; rsmix's [parse_tar] hands the
//! handler complete entries, and the streaming view is [TarParser::write]'s events. So
//! where Remix checks that an unfinished body reader rejects, these tests check that the
//! handler never saw the incomplete entry and that the events stop without an `End`.
//!
//! Not ported, because the types rule them out: "rejects invalid limits when constructing
//! the parser" and "rejects invalid options before reading the archive" (limits are
//! `Option<u64>`, so they cannot be negative, fractional or NaN, and `None` is Remix's
//! `Infinity`), and "rejects an unfinished reader when the source fails" (the caller
//! iterates its own source, so a failing source never reaches the parser and there is no
//! reader to reject). That is 70 of 73 cases.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::*;
use rsmix_tar_parser::*;

struct Counting;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
    static TOTAL: Cell<usize> = const { Cell::new(0) };
}

fn record(size: usize) {
    let _ = TRACKING.try_with(|tracking| {
        if tracking.get() {
            LARGEST.with(|largest| largest.set(largest.get().max(size)));
            TOTAL.with(|total| total.set(total.get() + size));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Run `f` and report (result, largest single allocation, total bytes allocated).
fn allocations<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    LARGEST.with(|c| c.set(0));
    TOTAL.with(|c| c.set(0));
    TRACKING.with(|c| c.set(true));
    let result = f();
    TRACKING.with(|c| c.set(false));
    (result, LARGEST.with(Cell::get), TOTAL.with(Cell::get))
}

const MIB: u64 = 1024 * 1024;

fn opts() -> TarOptions {
    TarOptions::default()
}

fn max_entry(size: u64) -> TarOptions {
    TarOptions { max_entry_size: Some(size), ..opts() }
}

fn unlimited_entries() -> TarOptions {
    TarOptions { max_entry_size: None, ..opts() }
}

fn pax_size(size: &str, kind: char) -> Vec<Vec<u8>> {
    pax("size", size, kind)
}

fn zeros(n: usize) -> Vec<u8> {
    vec![0; n]
}

fn invalid_size() -> TarError {
    invalid("Invalid tar entry size")
}

fn end_of_archive() -> TarError {
    invalid("Unexpected end of archive")
}

/// Remix's `assertInvalidArchive`: an empty file header follows the given chunks.
fn assert_invalid_archive(chunks: Vec<Vec<u8>>) {
    let chunks = concat(vec![chunks, vec![sized(0u64, '0')]]);
    let (entries, error) = parse_all(&chunks, &opts());
    assert_eq!(error, Some(invalid_size()));
    assert!(entries.is_empty());
}

/// Remix's `assertNoDeclaredAllocation`: a huge declared size allocates nothing near it.
fn assert_no_declared_allocation(chunks: Vec<Vec<u8>>) {
    let ((entries, error), largest, _) = allocations(|| parse_all(&chunks, &unlimited_entries()));
    assert_eq!(error, Some(end_of_archive()));
    assert!(entries.is_empty(), "the unfinished entry never reaches the handler");
    assert!(largest <= 4096, "largest allocation was {largest} bytes");
}

fn sizes(chunks: &[Vec<u8>], options: &TarOptions) -> (Vec<u64>, Option<TarError>) {
    let (entries, error) = parse_all(chunks, options);
    for entry in &entries {
        let expected = if entry.header.kind == TarEntryType::Directory { 0 } else { entry.size() };
        assert_eq!(entry.body.len() as u64, expected, "the body holds exactly the entry's size");
    }
    (entries.iter().map(TarEntry::size).collect(), error)
}

fn parse_all_slices(chunks: &[&[u8]]) -> Vec<TarEntry> {
    let mut entries = Vec::new();
    parse_tar_chunks(chunks, &opts(), |e| entries.push(e)).unwrap();
    entries
}

/// A source that counts how many chunks the parser pulled.
fn counted(chunks: Vec<Vec<u8>>, reads: &Cell<usize>) -> impl Iterator<Item = Vec<u8>> + '_ {
    chunks.into_iter().inspect(move |_| reads.set(reads.get() + 1))
}

// tar entry sizes

#[test]
fn does_not_allocate_the_declared_octal_size() {
    assert_no_declared_allocation(vec![sized(1024u64.pow(3), '0'), zeros(512)]);
}

#[test]
fn does_not_allocate_the_declared_base_256_size() {
    assert_no_declared_allocation(vec![sized(Size::Raw(base256(1 << 40)), '0'), zeros(512)]);
}

#[test]
fn does_not_allocate_the_declared_pax_size() {
    assert_no_declared_allocation(concat(vec![pax_size("9007199254740991", 'x'), vec![sized(0u64, '0')]]));
}

#[test]
fn rejects_non_octal_sizes_before_delivering_entries() {
    assert_invalid_archive(vec![sized("9", '0')]);
}

#[test]
fn rejects_partially_numeric_octal_sizes() {
    assert_eq!(parse_tar_header(&sized("17x", '0'), &opts()), Err(invalid_size()));
}

#[test]
fn rejects_negative_base_256_sizes_before_delivering_entries() {
    let mut size = vec![255u8; 12];
    size[10] = 253;
    assert_invalid_archive(vec![sized(Size::Raw(size), '0')]);
}

#[test]
fn rejects_base_256_negative_one_instead_of_decoding_it_as_zero() {
    assert_invalid_archive(vec![sized(Size::Raw(vec![255; 12]), '0')]);
}

#[test]
fn rejects_unsupported_base_256_markers() {
    let mut size = base256(0);
    size[0] = 0x81;
    assert_eq!(parse_tar_header(&sized(Size::Raw(size), '0'), &opts()), Err(invalid_size()));
}

#[test]
fn rejects_base_256_sizes_outside_the_safe_integer_range() {
    assert_eq!(parse_tar_header(&sized(Size::Raw(base256(1 << 53)), '0'), &opts()), Err(invalid_size()));
}

#[test]
fn rejects_invalid_extension_header_sizes() {
    assert_invalid_archive(vec![sized("9", 'x')]);
}

#[test]
fn rejects_nonnumeric_pax_sizes() {
    assert_invalid_archive(concat(vec![pax_size("unknown", 'x'), vec![sized(0u64, '0')]]));
}

#[test]
fn rejects_negative_pax_sizes() {
    assert_invalid_archive(concat(vec![pax_size("-512", 'x'), vec![sized(0u64, '0')]]));
}

#[test]
fn rejects_fractional_pax_sizes() {
    assert_invalid_archive(concat(vec![pax_size("1.5", 'x'), vec![sized(0u64, '0')]]));
}

#[test]
fn rejects_partially_numeric_pax_sizes() {
    assert_invalid_archive(concat(vec![pax_size("12x", 'x'), vec![sized(0u64, '0')]]));
}

#[test]
fn rejects_pax_sizes_outside_the_safe_integer_range() {
    assert_invalid_archive(concat(vec![pax_size("9007199254740992", 'x'), vec![sized(0u64, '0')]]));
}

#[test]
fn preserves_empty_and_padded_octal_size_fields() {
    let size = |field: &str| parse_tar_header(&sized(field, '0'), &opts()).unwrap().size;
    assert_eq!(size(""), 0);
    assert_eq!(size("            "), 0);
    assert_eq!(size("   0000014 "), 12);
    assert_eq!(size("\0\x0014\0"), 12);
    assert_eq!(size("777777777777"), 0o777777777777);
}

#[test]
fn preserves_safe_base_256_sizes_and_signed_metadata() {
    let mut block = sized(Size::Raw(base256((1 << 53) - 1)), '0');
    block[136..148].fill(255);
    block[147] = 253;
    set_checksum(&mut block);
    let header = parse_tar_header(&block, &opts()).unwrap();
    assert_eq!(header.size, (1 << 53) - 1);
    assert!(header.mtime.unwrap() < 0);
}

#[test]
fn uses_pax_sizes_for_content_and_padding() {
    let chunks = concat(vec![pax_size("0005", 'x'), vec![sized(0u64, '0'), b"hello".to_vec(), zeros(507), sized(0u64, '0')]]);
    let texts: Vec<String> = entries(&chunks, &opts()).iter().map(TarEntry::text).collect();
    assert_eq!(texts, ["hello", ""]);
}

#[test]
fn preserves_zero_pax_sizes_and_empty_pax_size_overrides() {
    let chunks = concat(vec![pax_size("0", 'x'), vec![sized(12u64, '0')], pax_size("", 'x'), vec![sized(0u64, '0')]]);
    assert_eq!(sizes(&chunks, &opts()), (vec![0, 0], None));
}

#[test]
fn uses_a_valid_pax_size_instead_of_the_overridden_header_field() {
    let chunks = concat(vec![pax_size("5", 'x'), vec![sized("unknown", '0'), b"hello".to_vec(), zeros(507)]]);
    let texts: Vec<String> = entries(&chunks, &opts()).iter().map(TarEntry::text).collect();
    assert_eq!(texts, ["hello"]);
}

#[test]
fn validates_extension_header_sizes_even_when_a_pax_override_is_pending() {
    assert_invalid_archive(concat(vec![pax_size("0", 'x'), vec![sized("unknown", 'L')]]));
}

// tar size limits

#[test]
fn exposes_resolved_defaults() {
    let parser = TarParser::new(TarOptions::default());
    assert_eq!(parser.options().max_entry_size, Some(2 * MIB));
    assert_eq!(parser.options().max_total_size, Some(20 * MIB));
    assert_eq!(parser.options().max_entries, Some(5000));
}

#[test]
fn limits_entries_to_2_mib_by_default_before_invoking_the_handler() {
    let (entries, error) = parse_all(&[sized(2 * MIB + 1, '0')], &opts());
    assert_eq!(error, Some(TarError::MaxEntrySizeExceeded(2 * MIB)));
    assert!(entries.is_empty());
}

#[test]
fn enforces_a_configured_entry_limit() {
    assert_eq!(error(&[sized(5u64, '0')], &max_entry(4)), TarError::MaxEntrySizeExceeded(4));
}

#[test]
fn counts_headers_and_padding_toward_the_total_limit() {
    let options = TarOptions { max_total_size: Some(1023), ..opts() };
    assert_eq!(error(&[sized(1u64, '0'), zeros(512)], &options), TarError::MaxTotalSizeExceeded(1023));
}

#[test]
fn allows_entry_and_total_sizes_exactly_at_their_limits() {
    let options = TarOptions { max_entry_size: Some(5), max_total_size: Some(1024), ..opts() };
    let texts: Vec<String> = entries(&[sized(5u64, '0'), b"hello".to_vec(), zeros(507)], &options).iter().map(TarEntry::text).collect();
    assert_eq!(texts, ["hello"]);
}

#[test]
fn allows_entries_exactly_at_the_default_limit_and_explicit_larger_limits() {
    let chunk = zeros(MIB as usize);
    let mut chunks = vec![sized(2 * MIB, '0'), chunk.clone(), chunk.clone()];
    assert_eq!(parse_all(&chunks, &opts()).1, None);
    chunks[0] = sized(3 * MIB, '0');
    chunks.push(chunk);
    assert_eq!(parse_all(&chunks, &max_entry(3 * MIB)).1, None);
}

#[test]
fn allows_only_empty_bodies_with_a_zero_entry_limit() {
    let entries = entries(&[sized(0u64, '0')], &max_entry(0));
    assert_eq!(entries.len(), 1);
    assert!(entries[0].body.is_empty());
    assert_eq!(error(&[sized(1u64, '0')], &max_entry(0)), TarError::MaxEntrySizeExceeded(0));
}

#[test]
fn allows_only_empty_input_with_a_zero_total_limit() {
    let options = TarOptions { max_total_size: Some(0), ..opts() };
    assert_eq!(parse_all(&[Vec::new()], &options).1, None);
    assert_eq!(error(&[zeros(1)], &options), TarError::MaxTotalSizeExceeded(0));
}

#[test]
fn checks_base_256_entry_sizes() {
    assert_eq!(error(&[sized(Size::Raw(base256(513)), '0')], &max_entry(512)), TarError::MaxEntrySizeExceeded(512));
}

#[test]
fn checks_the_effective_pax_size_instead_of_the_overridden_header_field() {
    let chunks = concat(vec![pax_size("513", 'x'), vec![sized(0u64, '0')]]);
    assert_eq!(error(&chunks, &max_entry(512)), TarError::MaxEntrySizeExceeded(512));
    let chunks = concat(vec![pax_size("0", 'x'), vec![sized(513u64, '0')]]);
    assert_eq!(sizes(&chunks, &max_entry(512)), (vec![0], None));
}

#[test]
fn limits_pax_and_gnu_metadata_before_reading_their_bodies() {
    for kind in ['x', 'g', 'L', 'K'] {
        let reads = Cell::new(0);
        let mut entries = 0;
        let result = parse_tar_chunks(counted(vec![sized(513u64, kind), zeros(1024)], &reads), &max_entry(512), |_| entries += 1);
        assert_eq!(result, Err(TarError::MaxEntrySizeExceeded(512)), "{kind}");
        assert_eq!((reads.get(), entries), (1, 0), "{kind}");
    }
}

#[test]
fn checks_global_pax_sizes_without_a_local_pax_header() {
    let (entries, error) = parse_all(&concat(vec![pax_size("513", 'g'), vec![sized(0u64, '0')]]), &max_entry(512));
    assert_eq!(error, Some(TarError::MaxEntrySizeExceeded(512)));
    assert!(entries.is_empty());
}

#[test]
fn preserves_global_pax_sizes_across_files_and_unrelated_global_updates() {
    let chunks = concat(vec![
        pax_size("1", 'g'),
        pax_size("0", 'x'),
        vec![sized(513u64, '0'), sized(0u64, '0'), zeros(512)],
        pax("uid", "123", 'g'),
        vec![sized(0u64, '0'), zeros(512)],
    ]);
    assert_eq!(sizes(&chunks, &max_entry(512)), (vec![0, 1, 1], None));
}

#[test]
fn allows_local_deletion_of_a_global_pax_size_for_one_entry() {
    let chunks = concat(vec![pax_size("513", 'g'), pax_size("", 'x'), vec![sized(0u64, '0'), sized(0u64, '0')]]);
    assert_eq!(sizes(&chunks, &max_entry(512)), (vec![0], Some(TarError::MaxEntrySizeExceeded(512))));
}

#[test]
fn allows_global_deletion_of_a_previous_pax_size() {
    let chunks = concat(vec![pax_size("513", 'g'), pax_size("", 'g'), vec![sized(0u64, '0'), sized(0u64, '0')]]);
    assert_eq!(sizes(&chunks, &max_entry(512)), (vec![0, 0], None));
}

#[test]
fn does_not_let_a_pax_override_bypass_a_metadata_entry_limit() {
    let chunks = concat(vec![pax_size("0", 'x'), vec![sized(513u64, 'L')]]);
    assert_eq!(error(&chunks, &max_entry(512)), TarError::MaxEntrySizeExceeded(512));
}

#[test]
fn counts_extension_metadata_toward_the_total_limit() {
    let options = TarOptions { max_total_size: Some(1535), ..opts() };
    let (entries, error) = parse_all(&concat(vec![pax_size("0", 'x'), vec![sized(0u64, '0')]]), &options);
    assert_eq!(error, Some(TarError::MaxTotalSizeExceeded(1535)));
    assert!(entries.is_empty());
}

#[test]
fn limits_total_input_to_20_mib_and_allows_raising_or_disabling_the_limit() {
    let chunk = zeros(MIB as usize);
    let mut chunks = vec![sized(20 * MIB - 512, '0')];
    chunks.extend(std::iter::repeat_n(chunk.clone(), 19));
    chunks.push(chunk[512..].to_vec());
    let count = |chunks: &[Vec<u8>], options: &TarOptions| parse_tar_chunks(chunks, options, |_| {});
    assert_eq!(count(&chunks, &unlimited_entries()), Ok(()));
    chunks.push(zeros(512));
    assert_eq!(count(&chunks, &unlimited_entries()), Err(TarError::MaxTotalSizeExceeded(20 * MIB)));
    assert_eq!(count(&chunks, &TarOptions { max_total_size: Some(20 * MIB + 512), ..unlimited_entries() }), Ok(()));
    assert_eq!(count(&chunks, &TarOptions { max_total_size: None, ..unlimited_entries() }), Ok(()));
}

#[test]
fn rejects_an_oversized_input_chunk_before_invoking_any_handlers() {
    let (entries, error) = parse_all(&[sized(0u64, '0')], &TarOptions { max_total_size: Some(511), ..opts() });
    assert_eq!(error, Some(TarError::MaxTotalSizeExceeded(511)));
    assert!(entries.is_empty());
}

#[test]
fn limits_archives_to_5000_entries_by_default_before_invoking_the_next_handler() {
    let (entries, error) = parse_all(&vec![sized(0u64, '0'); 5001], &opts());
    assert_eq!(error, Some(TarError::MaxEntriesExceeded(5000)));
    assert_eq!(entries.len(), 5000);
}

#[test]
fn allows_exactly_5000_entries_and_explicit_larger_or_disabled_count_limits() {
    let mut chunks = vec![sized(0u64, '0'); 5000];
    assert_eq!(entries(&chunks, &opts()).len(), 5000);
    chunks.push(sized(0u64, '0'));
    assert_eq!(entries(&chunks, &TarOptions { max_entries: Some(5001), ..opts() }).len(), 5001);
    assert_eq!(entries(&chunks, &TarOptions { max_entries: None, ..opts() }).len(), 5001);
}

#[test]
fn allows_empty_archives_and_end_markers_with_a_zero_entry_count_limit() {
    let options = TarOptions { max_entries: Some(0), ..opts() };
    assert_eq!(parse_all(&[Vec::new()], &options).1, None);
    assert_eq!(parse_all(&[zeros(1024)], &options).1, None);
    assert_eq!(error(&[sized(0u64, '0')], &options), TarError::MaxEntriesExceeded(0));
}

#[test]
fn counts_files_directories_and_links_toward_the_entry_count_limit() {
    let chunks = vec![sized(0u64, '0'), sized(0u64, '5'), sized(0u64, '2')];
    let (entries, error) = parse_all(&chunks, &TarOptions { max_entries: Some(2), ..opts() });
    assert_eq!(error, Some(TarError::MaxEntriesExceeded(2)));
    let kinds: Vec<&str> = entries.iter().map(|e| e.header.kind.as_str()).collect();
    assert_eq!(kinds, ["file", "directory"]);
}

#[test]
fn stops_at_excess_metadata_entries_before_reading_their_bodies() {
    for kind in ['x', 'g', 'L', 'K'] {
        let reads = Cell::new(0);
        let mut entries = 0;
        let source = counted(vec![sized(0u64, '0'), sized(1u64, kind), zeros(512)], &reads);
        let result = parse_tar_chunks(source, &TarOptions { max_entries: Some(1), ..opts() }, |_| entries += 1);
        assert_eq!(result, Err(TarError::MaxEntriesExceeded(1)), "{kind}");
        assert_eq!((reads.get(), entries), (2, 1), "{kind}");
    }
}

#[test]
fn counts_metadata_entries_even_when_no_entries_reach_the_handler() {
    let (entries, error) = parse_all(&concat(vec![pax_size("0", 'g'), pax_size("0", 'x')]), &TarOptions { max_entries: Some(1), ..opts() });
    assert_eq!(error, Some(TarError::MaxEntriesExceeded(1)));
    assert!(entries.is_empty());
}

#[test]
fn counts_split_headers_once_and_excludes_bodies_padding_and_end_markers() {
    let header = sized(513u64, '0');
    let chunks = vec![header[..256].to_vec(), header[256..].to_vec(), vec![1; 513], zeros(511), sized(0u64, '0'), zeros(1024)];
    let mut lengths: Vec<usize> = entries(&chunks, &TarOptions { max_entries: Some(2), ..opts() }).iter().map(|e| e.body.len()).collect();
    lengths.sort();
    assert_eq!(lengths, [0, 513]);
}

#[test]
fn does_not_reset_the_entry_count_at_zero_blocks_between_entries() {
    let chunks = vec![sized(0u64, '0'), zeros(512), sized(0u64, '0')];
    let (entries, error) = parse_all(&chunks, &TarOptions { max_entries: Some(1), ..opts() });
    assert_eq!(error, Some(TarError::MaxEntriesExceeded(1)));
    assert_eq!(entries.len(), 1);
}

#[test]
fn continues_parsing_after_nonconsecutive_zero_blocks() {
    let chunks = vec![sized(0u64, '0'), zeros(512), sized(0u64, '0'), zeros(512), sized(0u64, '0')];
    let (entries, error) = parse_all(&chunks, &opts());
    assert_eq!(error, None);
    assert_eq!(entries.len(), 3);
}

#[test]
fn stops_parsing_entries_after_two_zero_blocks() {
    let mut archive = zeros(2048);
    archive[..512].copy_from_slice(&sized(0u64, '0'));
    archive[1536..].copy_from_slice(&sized(0u64, '0'));
    let (entries, error) = parse_all(&[archive], &opts());
    assert_eq!(error, None);
    assert_eq!(entries.len(), 1);
}

#[test]
fn stops_parsing_entries_after_a_chunk_split_end_marker() {
    let chunks = vec![sized(0u64, '0'), zeros(700), zeros(324), sized(0u64, '0')];
    let (entries, error) = parse_all(&chunks, &opts());
    assert_eq!(error, None);
    assert_eq!(entries.len(), 1);
}

#[test]
fn counts_trailing_input_after_the_end_marker_toward_the_total_size_limit() {
    let chunks = vec![sized(0u64, '0'), zeros(1024), zeros(1)];
    let (entries, error) = parse_all(&chunks, &TarOptions { max_total_size: Some(1536), ..opts() });
    assert_eq!(error, Some(TarError::MaxTotalSizeExceeded(1536)));
    assert_eq!(entries.len(), 1);
}

#[test]
fn resets_end_marker_state_between_parses() {
    let mut parser = TarParser::new(opts());
    let mut entries = 0;
    assert_eq!(parser.parse([sized(0u64, '0'), zeros(1024)], |_| entries += 1), Ok(()));
    assert_eq!(parser.parse([sized(0u64, '0')], |_| entries += 1), Ok(()));
    assert_eq!(entries, 2);
}

#[test]
fn resets_entry_accounting_between_successful_and_failed_parses() {
    let mut parser = TarParser::new(TarOptions { max_entries: Some(1), ..opts() });
    let mut entries = 0;
    let one = [sized(0u64, '0')];
    assert_eq!(parser.parse(&one, |_| entries += 1), Ok(()));
    assert_eq!(parser.parse(&one, |_| entries += 1), Ok(()));
    assert_eq!(parser.parse([&one[0], &one[0]], |_| entries += 1), Err(TarError::MaxEntriesExceeded(1)));
    assert_eq!(parser.parse(&one, |_| entries += 1), Ok(()));
    assert_eq!(entries, 4);
}

#[test]
fn resets_total_accounting_between_successful_and_failed_parses() {
    let mut parser = TarParser::new(TarOptions { max_total_size: Some(512), ..opts() });
    let mut entries = 0;
    let one = [sized(0u64, '0')];
    assert_eq!(parser.parse(&one, |_| entries += 1), Ok(()));
    assert_eq!(parser.parse(&one, |_| entries += 1), Ok(()));
    assert_eq!(parser.parse([one[0].clone(), zeros(1)], |_| entries += 1), Err(TarError::MaxTotalSizeExceeded(512)));
    assert_eq!(parser.parse(&one, |_| entries += 1), Ok(()));
    assert_eq!(entries, 4);
}

#[test]
fn stops_a_source_at_the_total_limit_without_delivering_the_unfinished_entry() {
    let reads = Cell::new(0);
    let source = std::iter::once(sized(2u64, '0')).chain(std::iter::repeat_with(|| {
        reads.set(reads.get() + 1);
        vec![1]
    }));
    let mut entries = 0;
    let result = parse_tar_chunks(source, &TarOptions { max_total_size: Some(513), ..opts() }, |_| entries += 1);
    assert_eq!(result, Err(TarError::MaxTotalSizeExceeded(513)));
    assert_eq!(reads.get(), 2);
    assert_eq!(entries, 0);
}

#[test]
fn enforces_the_total_limit_on_streams_before_forwarding_excess_bytes() {
    let chunks = [sized(2u64, '0'), vec![1], vec![2]];
    let mut parser = TarParser::new(TarOptions { max_total_size: Some(513), ..opts() });
    let mut received = Vec::new();
    let mut reads = 0;
    let mut result = Ok(());
    for chunk in &chunks {
        reads += 1;
        result = parser.write(chunk, &mut |event| {
            if let TarEvent::Data(bytes) = event {
                received.extend_from_slice(bytes);
            }
        });
        if result.is_err() {
            break;
        }
    }
    assert_eq!(result, Err(TarError::MaxTotalSizeExceeded(513)));
    assert_eq!(received, [1]);
    assert_eq!(reads, 3);
}

#[test]
fn exports_named_limit_errors() {
    assert_eq!(TarError::MaxEntrySizeExceeded(10).to_string(), "Tar entry size exceeds maximum allowed size of 10 bytes");
    assert_eq!(TarError::MaxTotalSizeExceeded(100).to_string(), "Tar archive size exceeds maximum allowed size of 100 bytes");
    assert_eq!(TarError::MaxEntriesExceeded(5000).to_string(), "Tar entry count exceeds maximum allowed count of 5000");
}

// tar metadata buffering

/// Remix's `assertLinearBuffering`: 64 KiB of metadata in 64-byte chunks.
fn assert_linear_buffering(kind: char, key: &str) {
    let value = "a".repeat(64 * 1024);
    let chunks = if kind == 'x' || kind == 'g' { pax(key, &value, kind) } else { vec![sized(value.len() as u64, kind), value.clone().into_bytes()] };
    let mut pieces: Vec<Vec<u8>> = chunks.iter().flat_map(|c| c.chunks(64).map(<[u8]>::to_vec)).collect();
    pieces.push(sized(0u64, '0'));
    let (entries, _, total) = allocations(|| entries(&pieces, &opts()));
    let found: Vec<String> = entries.into_iter().map(|e| if key == "path" { e.header.name } else { e.header.linkname.unwrap_or_default() }).collect();
    assert_eq!(found, std::slice::from_ref(&value));
    // Every allocation is counted: the metadata buffer's doubling growth (about 4x, Remix's
    // whole bound), the decoded value, the header name, the entry's clone and the global
    // merge. Measured: under 4.1x for GNU entries, about 8.1x local and 9.1x global PAX.
    // Re-buffering on every 64-byte chunk would be quadratic, about 500x.
    assert!(total < 10 * value.len(), "allocated {total} bytes for {} bytes", value.len());
}

#[test]
fn buffers_fragmented_gnu_paths_with_linear_allocation() {
    assert_linear_buffering('L', "path");
}

#[test]
fn buffers_fragmented_gnu_link_targets_with_linear_allocation() {
    assert_linear_buffering('K', "linkpath");
}

#[test]
fn buffers_fragmented_local_pax_metadata_with_linear_allocation() {
    assert_linear_buffering('x', "path");
}

#[test]
fn buffers_fragmented_global_pax_metadata_with_linear_allocation() {
    assert_linear_buffering('g', "linkpath");
}

#[test]
fn allocates_metadata_storage_from_received_bytes_with_the_entry_limit_disabled() {
    let chunks = vec![sized(1024u64.pow(3), 'L'), zeros(1)];
    let ((entries, error), _, total) = allocations(|| parse_all(&chunks, &unlimited_entries()));
    assert_eq!(error, Some(end_of_archive()));
    assert!(entries.is_empty());
    assert!(total <= 4096, "allocated {total} bytes");
}

#[test]
fn preserves_split_utf8_metadata_and_following_content_across_padding_boundaries() {
    let name = "files/\u{e9}.txt";
    let body = name.as_bytes();
    let mut tail = zeros(1536 - 7);
    tail[..body.len() - 7].copy_from_slice(&body[7..]);
    tail[512 - 7..1024 - 7].copy_from_slice(&sized(2u64, '0'));
    tail[1024 - 7..1024 - 7 + 2].copy_from_slice(b"ok");
    let chunks = vec![sized(body.len() as u64, 'L'), body[..7].to_vec(), tail];
    let found: Vec<String> = entries(&chunks, &opts()).iter().map(|e| format!("{}: {}", e.name(), e.text())).collect();
    assert_eq!(found, [format!("{name}: ok")]);
}

#[test]
fn rejects_truncated_metadata_and_resets_buffering_before_reuse() {
    let mut parser = TarParser::new(opts());
    assert_eq!(parser.parse([sized(8u64, 'L'), b"part".to_vec()], |_| {}), Err(end_of_archive()));
    let mut names = Vec::new();
    let chunks = concat(vec![pax_path("next.txt", 'x'), vec![sized(0u64, '0')]]);
    parser.parse(&chunks, |e| names.push(e.header.name)).unwrap();
    assert_eq!(names, ["next.txt"]);
}

// TarEntry body readers

#[test]
fn delivers_received_bytes_independently_of_the_header_size() {
    let (events, result) = events(&[sized(10u64, '0'), vec![1, 2], vec![3]], &opts());
    assert_eq!(events, [Event::Entry("entry.txt".into()), Event::Data(vec![1, 2]), Event::Data(vec![3])]);
    assert_eq!(result, Err(end_of_archive()));
}

#[test]
fn returns_an_exact_independent_body() {
    let mut source = [0, 1, 2, 0];
    let entries = parse_all_slices(&[&sized(2u64, '0'), &source[1..3], &zeros(510)]);
    source.fill(9);
    assert_eq!(entries[0].body, [1, 2], "exactly the body bytes, unaffected by the source buffer");
}

#[test]
fn copies_each_chunk_from_a_source_that_reuses_its_buffer() {
    let mut parser = TarParser::new(opts());
    let mut entries = Vec::new();
    let mut body = Vec::new();
    let mut collect = |event: TarEvent<'_>| match event {
        TarEvent::Data(bytes) => body.extend_from_slice(bytes),
        TarEvent::End => entries.push(std::mem::take(&mut body)),
        TarEvent::Entry(_) => {}
    };
    parser.write(&sized(4u64, '0'), &mut collect).unwrap();
    let mut buffer = vec![1, 2];
    parser.write(&buffer, &mut collect).unwrap();
    buffer.copy_from_slice(&[3, 4]);
    parser.write(&buffer, &mut collect).unwrap();
    parser.write(&zeros(508), &mut collect).unwrap();
    parser.finish().unwrap();
    assert_eq!(entries, [vec![1, 2, 3, 4]]);
}

#[test]
fn decodes_utf8_split_across_chunks() {
    let bytes = "h\u{e9}llo".as_bytes();
    let chunks = vec![sized(bytes.len() as u64, '0'), bytes[..2].to_vec(), bytes[2..].to_vec(), zeros(512 - bytes.len())];
    let texts: Vec<String> = entries(&chunks, &opts()).iter().map(TarEntry::text).collect();
    assert_eq!(texts, ["h\u{e9}llo"]);
}

#[test]
fn returns_empty_content_for_directories_with_a_nonzero_size() {
    let entries = entries(&[sized(12u64, '5')], &opts());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].size(), 12);
    assert!(entries[0].body.is_empty());
}

#[test]
fn rejects_a_truncated_buffered_archive_without_delivering_the_entry() {
    let (entries, error) = parse_all(&[sized(12u64, '0')], &opts());
    assert_eq!(error, Some(end_of_archive()));
    assert!(entries.is_empty());
}

#[test]
fn settles_a_reader_of_a_truncated_source_stream() {
    let (events, result) = events(&[sized(12u64, '0'), vec![1, 2]], &opts());
    assert_eq!(events, [Event::Entry("entry.txt".into()), Event::Data(vec![1, 2])], "no End for the unfinished body");
    assert_eq!(result, Err(end_of_archive()));
}

#[test]
fn rejects_missing_padding_after_delivering_the_complete_body() {
    let (entries, error) = parse_all(&[sized(5u64, '0'), b"hello".to_vec()], &opts());
    assert_eq!(error, Some(end_of_archive()));
    let texts: Vec<String> = entries.iter().map(TarEntry::text).collect();
    assert_eq!(texts, ["hello"]);
}

#[test]
fn a_failing_handler_fails_the_parse() {
    let result = std::panic::catch_unwind(|| parse_tar(&sized(0u64, '0'), &opts(), |_| panic!("Handler failed")));
    let payload = result.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"Handler failed"));
}

#[test]
fn streams_entry_bytes_before_the_archive_finishes() {
    let mut parser = TarParser::new(opts());
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut collect = |event: TarEvent<'_>| {
        if let TarEvent::Data(bytes) = event {
            chunks.push(bytes.to_vec());
        }
    };
    parser.write(&sized(4u64, '0'), &mut collect).unwrap();
    parser.write(&[1, 2], &mut collect).unwrap();
    parser.write(&[3, 4], &mut collect).unwrap();
    parser.write(&zeros(508), &mut collect).unwrap();
    parser.finish().unwrap();
    assert_eq!(chunks, [vec![1, 2], vec![3, 4]]);
}
