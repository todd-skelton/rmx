use rsmix_fetch::block_on;
use serde_json::json;

use super::*;

/// Save, then read back the way the next request would.
fn round_trip(storage: &impl SessionStorage, session: &Session) -> (Option<String>, Session) {
    let cookie = block_on(storage.save(session)).unwrap();
    let next = block_on(storage.read(cookie.as_deref())).unwrap();
    (cookie, next)
}

#[test]
fn values_flash_and_dirty_tracking() {
    let s = Session::with_data("abc", SessionData::default());
    assert!(!s.is_dirty());
    assert_eq!(s.id(), "abc");
    s.set("name", "Ada");
    s.set("prefs", json!({ "theme": "dark" }));
    assert!(s.is_dirty());
    assert_eq!(s.get::<String>("name").as_deref(), Some("Ada"));
    assert_eq!(s.get::<i64>("name"), None, "wrong type reads as None");
    assert_eq!(s.get_value("prefs"), Some(json!({ "theme": "dark" })));
    s.set("name", None::<String>);
    assert!(!s.has("name"), "setting null removes");
    s.unset("prefs");
    assert!(s.is_empty());

    s.flash("message", "saved");
    assert!(!s.has("message"), "flash is for the next request");
    assert_eq!(s.data().1.get("message"), Some(&json!("saved")));
}

#[test]
fn flash_survives_exactly_one_request() {
    let storage = MemoryStorage::new();
    let s = block_on(storage.read(None)).unwrap();
    s.set("user", 1);
    s.flash("message", "hello");
    let (_, next) = round_trip(&storage, &s);
    assert_eq!(next.get::<String>("message").as_deref(), Some("hello"));
    assert!(next.is_dirty(), "loaded flash marks the session dirty so it gets cleared");
    let (_, after) = round_trip(&storage, &next);
    assert!(!after.has("message"));
    assert_eq!(after.get::<i64>("user"), Some(1));
}

#[test]
fn regenerate_id_and_destroy() {
    let storage = MemoryStorage::new();
    let s = Session::new();
    s.set("a", 1);
    let (old_cookie, loaded) = round_trip(&storage, &s);
    let old_id = old_cookie.unwrap();
    loaded.regenerate_id(true);
    assert_ne!(loaded.id(), old_id);
    assert_eq!(loaded.delete_id().as_deref(), Some(old_id.as_str()));
    let (new_cookie, _) = round_trip(&storage, &loaded);
    assert_eq!(new_cookie, Some(loaded.id()));
    let stale = block_on(storage.read(Some(&old_id))).unwrap();
    assert!(stale.is_empty() && stale.id() != old_id, "old id was deleted");

    let live = block_on(storage.read(new_cookie.as_deref())).unwrap();
    live.destroy();
    assert!(!live.has("a"));
    assert_eq!(block_on(storage.save(&live)).unwrap(), Some(String::new()));
    assert!(block_on(storage.read(new_cookie.as_deref())).unwrap().is_empty());
}

#[test]
#[should_panic(expected = "destroyed")]
fn set_after_destroy_panics() {
    let s = Session::new();
    s.destroy();
    s.set("a", 1);
}

#[test]
fn unchanged_sessions_are_not_saved() {
    let memory = MemoryStorage::new();
    assert_eq!(block_on(memory.save(&Session::new())).unwrap(), None);
    assert_eq!(block_on(CookieStorage.save(&Session::new())).unwrap(), None);
}

#[test]
fn memory_storage_unknown_ids() {
    assert_ne!(block_on(MemoryStorage::new().read(Some("forged"))).unwrap().id(), "forged");
    let reuse = MemoryStorage::new().use_unknown_ids(true);
    assert_eq!(block_on(reuse.read(Some("forged"))).unwrap().id(), "forged");
}

#[test]
fn cookie_storage_keeps_data_in_the_cookie() {
    let s = Session::with_data("id1", SessionData::default());
    s.set("cart", vec![1, 2]);
    let (cookie, next) = round_trip(&CookieStorage, &s);
    assert_eq!(cookie.as_deref(), Some(r#"{"i":"id1","d":[{"cart":[1,2]},{}]}"#));
    assert_eq!(next.id(), "id1");
    assert_eq!(next.get::<Vec<i32>>("cart"), Some(vec![1, 2]));
    assert_ne!(block_on(CookieStorage.read(Some("not json"))).unwrap().id(), "id1");
}

#[cfg(feature = "fs")]
#[test]
fn fs_storage_round_trips_and_deletes() {
    let dir = std::env::temp_dir().join(format!("rsmix-session-test-{}", rsmix_crypto::random_hex(8)));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let storage = FsStorage::new(&dir).unwrap();
        let s = storage.read(None).await.unwrap();
        s.set("n", 7);
        let id = storage.save(&s).await.unwrap().unwrap();
        let hash = rsmix_crypto::hex(&rsmix_crypto::sha256(id.as_bytes()));
        assert!(dir.join(&hash[..2]).join(&hash[2..]).is_file());
        let loaded = storage.read(Some(&id)).await.unwrap();
        assert_eq!(loaded.get::<i32>("n"), Some(7));
        loaded.destroy();
        assert_eq!(storage.save(&loaded).await.unwrap(), Some(String::new()));
        assert!(storage.read(Some(&id)).await.unwrap().is_empty());
    });
    let _ = std::fs::remove_dir_all(&dir);
}
