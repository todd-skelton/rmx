//! Remix's `session-storage/fs.test.ts`.

#![cfg(feature = "fs")]

mod common;

use std::path::PathBuf;

use rsmix_session::FsStorage;

/// A fresh directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("file-session-storage-test-{}", rsmix_crypto::random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn storage(&self) -> FsStorage {
        FsStorage::new(&self.0).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn does_not_use_unknown_session_ids_by_default() {
    let tmp = TempDir::new();
    assert_ne!(common::read(&tmp.storage(), Some("unknown")).id(), "unknown");
}

#[test]
fn uses_unknown_session_ids_if_enabled() {
    let tmp = TempDir::new();
    assert_eq!(common::read(&tmp.storage().use_unknown_ids(true), Some("unknown")).id(), "unknown");
}

#[test]
fn persists_session_data_across_requests() {
    common::persists_session_data_across_requests(&TempDir::new().storage());
}

#[test]
fn clears_session_data_when_the_session_is_destroyed() {
    common::clears_session_data_when_the_session_is_destroyed(&TempDir::new().storage());
}

#[test]
fn does_not_set_a_cookie_when_session_data_is_not_changed() {
    common::does_not_set_a_cookie_when_session_data_is_not_changed(&TempDir::new().storage());
}

#[test]
fn makes_flash_data_available_only_on_the_next_request() {
    common::makes_flash_data_available_only_on_the_next_request(&TempDir::new().storage());
}

#[test]
fn leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated() {
    common::leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated(&TempDir::new().storage());
}

#[test]
fn deletes_old_session_data_when_the_id_is_regenerated_and_delete_old_session_is_true() {
    common::deletes_old_session_data_when_the_id_is_regenerated_with_delete_old(&TempDir::new().storage());
}

#[test]
fn throws_error_if_session_directory_is_a_file() {
    let tmp = TempDir::new();
    let file = tmp.0.join("not-a-directory");
    std::fs::write(&file, "I am a file, not a directory.").unwrap();
    let error = FsStorage::new(&file).unwrap_err();
    assert_eq!(error.to_string(), format!("Path \"{}\" is not a directory", file.display()));
}
