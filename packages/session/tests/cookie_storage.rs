//! Remix's `session-storage/cookie.test.ts`. The regenerate-with-delete warning goes to
//! stderr, which the test harness does not capture for checking; the behavior around it is checked.

mod common;

use rsmix_session::CookieStorage;

#[test]
fn persists_session_data_across_requests() {
    common::persists_session_data_across_requests(&CookieStorage::new());
}

#[test]
fn clears_session_data_when_the_session_is_destroyed() {
    common::clears_session_data_when_the_session_is_destroyed(&CookieStorage::new());
}

#[test]
fn does_not_set_a_cookie_when_session_data_is_not_changed() {
    common::does_not_set_a_cookie_when_session_data_is_not_changed(&CookieStorage::new());
}

#[test]
fn makes_flash_data_available_only_on_the_next_request() {
    common::makes_flash_data_available_only_on_the_next_request(&CookieStorage::new());
}

#[test]
fn leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated() {
    common::leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated(&CookieStorage::new());
}

#[test]
fn keeps_old_cookies_working_when_the_id_is_regenerated_with_delete_old() {
    let storage = CookieStorage::new();
    let r1 = common::count(&storage, None);
    let r2 = common::login(&storage, r1.cookie.as_deref(), true);
    assert_ne!(r2.session.id(), r1.session.id());
    let r3 = common::count(&storage, r2.cookie.as_deref());
    assert_eq!(r3.session.get::<i64>("count"), Some(2));
}
