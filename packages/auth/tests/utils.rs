//! Port of Remix's `utils.test.ts` (`sanitizeReturnTo()`).

use rsmix_auth::sanitize_return_to;

#[test]
fn drops_paths_that_normalize_to_an_authority_reference() {
    for value in ["/..//other.example", "/.//other.example", "/%2e%2e//other.example", "/account/..//other.example", "/..\\/other.example", "/..//remix.local"] {
        assert_eq!(sanitize_return_to(Some(value)), None, "{value}");
    }
}

#[test]
fn preserves_local_paths_queries_fragments_and_encoded_path_data() {
    assert_eq!(sanitize_return_to(Some("/account/../dashboard?tab=profile#name")).as_deref(), Some("/dashboard?tab=profile#name"));
    assert_eq!(sanitize_return_to(Some("/account//settings")).as_deref(), Some("/account//settings"));
    assert_eq!(sanitize_return_to(Some("/%2Fother.example")).as_deref(), Some("/%2Fother.example"));
    assert_eq!(sanitize_return_to(Some("/?next=//other.example#//section")).as_deref(), Some("/?next=//other.example#//section"));
}
