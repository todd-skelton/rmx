//! Port of data-schema's pipe.test.ts.

use rsmix_data_schema::checks::{max_length, min_length};
use rsmix_data_schema::{self as s, Schema};

#[test]
fn applies_checks_in_order() {
    let schema = s::string().pipe(min_length(2)).pipe(max_length(4));
    assert!(s::parse(&schema, "test").is_ok());
    let short = s::parse(&schema, "a").unwrap_err().issues;
    let long = s::parse(&schema, "toolong").unwrap_err().issues;
    assert_eq!(short[0].message, "Expected at least 2 characters");
    assert_eq!(long[0].message, "Expected at most 4 characters");
}
