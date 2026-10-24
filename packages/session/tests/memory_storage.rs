//! Remix's `session-storage/memory.test.ts`.

mod common;

use rsmix_session::MemoryStorage;

#[test]
fn does_not_use_unknown_session_ids_by_default() {
    assert_ne!(common::read(&MemoryStorage::new(), Some("unknown")).id(), "unknown");
}

#[test]
fn uses_unknown_session_ids_if_enabled() {
    assert_eq!(common::read(&MemoryStorage::new().use_unknown_ids(true), Some("unknown")).id(), "unknown");
}

#[test]
fn persists_session_data_across_requests() {
    common::persists_session_data_across_requests(&MemoryStorage::new());
}

#[test]
fn clears_session_data_when_the_session_is_destroyed() {
    common::clears_session_data_when_the_session_is_destroyed(&MemoryStorage::new());
}

#[test]
fn does_not_set_a_cookie_when_session_data_is_not_changed() {
    common::does_not_set_a_cookie_when_session_data_is_not_changed(&MemoryStorage::new());
}

#[test]
fn makes_flash_data_available_only_on_the_next_request() {
    common::makes_flash_data_available_only_on_the_next_request(&MemoryStorage::new());
}

#[test]
fn leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated() {
    common::leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated(&MemoryStorage::new());
}

#[test]
fn deletes_old_session_data_when_the_id_is_regenerated_and_delete_old_session_is_true() {
    common::deletes_old_session_data_when_the_id_is_regenerated_with_delete_old(&MemoryStorage::new());
}
