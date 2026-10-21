//! Port of Remix's `byte-range.test.ts`. `Infinity` is `ByteRange::END`.

use rmx_lazy_file::{get_byte_length, get_indexes, ByteRange};

const INF: i64 = ByteRange::END;

#[test]
fn get_byte_length_returns_the_correct_length() {
    let size = 100;
    assert_eq!(get_byte_length(ByteRange::new(10, 20), size), 10);
    assert_eq!(get_byte_length(ByteRange::new(10, -10), size), 80);
    assert_eq!(get_byte_length(ByteRange::new(-10, -10), size), 0);
    assert_eq!(get_byte_length(ByteRange::new(-10, 20), size), 0);
    assert_eq!(get_byte_length(ByteRange::new(0, INF), size), 100);
    assert_eq!(get_byte_length(ByteRange::new(INF, 0), size), 0);
    assert_eq!(get_byte_length(ByteRange::new(INF, INF), size), 0);
}

#[test]
fn get_indexes_returns_the_correct_indexes() {
    let size = 100;
    assert_eq!(get_indexes(ByteRange::new(10, 20), size), (10, 20));
    assert_eq!(get_indexes(ByteRange::new(10, -10), size), (10, 90));
    assert_eq!(get_indexes(ByteRange::new(-10, -10), size), (90, 90));
    assert_eq!(get_indexes(ByteRange::new(-10, 20), size), (90, 90));
    assert_eq!(get_indexes(ByteRange::new(0, INF), size), (0, 100));
    assert_eq!(get_indexes(ByteRange::new(INF, 0), size), (100, 100));
    assert_eq!(get_indexes(ByteRange::new(INF, INF), size), (100, 100));
}
