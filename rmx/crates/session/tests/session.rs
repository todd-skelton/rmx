//! Remix's `session.test.ts`. A destroyed session panics where Remix throws.

use rmx_session::{Session, SessionData};
use serde_json::{json, Map, Value};

fn data(values: Value, flash: Value) -> SessionData {
    let map = |v: Value| -> Map<String, Value> { v.as_object().cloned().unwrap_or_default() };
    SessionData(map(values), map(flash))
}

#[test]
fn creates_a_new_session() {
    let session = Session::new();
    assert!(!session.id().is_empty());
    assert!(!session.is_destroyed());
    assert!(!session.is_dirty());
    assert_eq!(session.len(), 0);
}

#[test]
fn creates_a_new_session_with_a_custom_id() {
    assert_eq!(Session::with_data("custom-id", SessionData::default()).id(), "custom-id");
}

#[test]
fn creates_a_new_session_with_initial_data() {
    let session = Session::with_data(rmx_session::create_session_id(), data(json!({ "hello": "world" }), json!({})));
    assert_eq!(session.len(), 1);
    assert_eq!(session.get::<String>("hello").as_deref(), Some("world"));
    assert!(!session.is_dirty());
}

#[test]
fn creates_a_new_session_with_initial_flash_data() {
    let session = Session::with_data(rmx_session::create_session_id(), data(json!({}), json!({ "hello": "world" })));
    assert_eq!(session.get::<String>("hello").as_deref(), Some("world"));
    assert!(session.is_dirty());
}

#[test]
fn sets_and_gets_values() {
    let session = Session::new();
    assert!(!session.has("hello"));
    assert_eq!(session.get_value("hello"), None);
    assert!(!session.is_dirty());

    session.set("hello", "world");
    assert!(session.has("hello"));
    assert_eq!(session.get::<String>("hello").as_deref(), Some("world"));
    assert!(session.is_dirty());

    session.unset("hello");
    assert!(!session.has("hello"));
    assert_eq!(session.get_value("hello"), None);
    assert!(session.is_dirty());
}

#[test]
fn sets_and_gets_values_with_complex_types() {
    let session = Session::with_data("id", data(json!({ "user": { "id": 123, "name": "alice" } }), json!({})));
    assert_eq!(session.get_value("user"), Some(json!({ "id": 123, "name": "alice" })));
    session.set("user", json!({ "id": 456, "name": "bob" }));
    assert_eq!(session.get_value("user"), Some(json!({ "id": 456, "name": "bob" })));
}

#[test]
fn unsets_values_when_set_to_null() {
    let session = Session::new();
    session.set("hello", "world");
    assert!(session.has("hello"));
    assert!(session.is_dirty());
    session.set("hello", Value::Null);
    assert!(!session.has("hello"));
    assert_eq!(session.get_value("hello"), None);
    assert!(session.is_dirty());
}

#[test]
fn unsets_values_when_set_to_none() {
    let session = Session::new();
    session.set("hello", "world");
    session.set("hello", None::<String>);
    assert!(!session.has("hello"));
    assert_eq!(session.get_value("hello"), None);
    assert!(session.is_dirty());
}

#[test]
fn regenerates_the_session_id() {
    let session = Session::new();
    let original = session.id();
    assert!(!session.is_dirty());
    session.regenerate_id(false);
    assert_ne!(session.id(), original);
    assert_eq!(session.delete_id(), None);
    assert!(session.is_dirty());
}

#[test]
fn deletes_the_old_session_id() {
    let session = Session::new();
    let original = session.id();
    session.regenerate_id(true);
    assert_ne!(session.id(), original);
    assert_eq!(session.delete_id(), Some(original));
    assert!(session.is_dirty());
}

#[test]
fn destroys_the_session() {
    let session = Session::new();
    assert!(!session.is_destroyed());
    session.destroy();
    assert!(session.is_destroyed());
}

#[test]
fn deletes_the_original_session_when_the_session_id_is_regenerated_more_than_once() {
    let session = Session::new();
    let original = session.id();
    session.regenerate_id(true);
    assert_ne!(session.id(), original);
    assert_eq!(session.delete_id().as_deref(), Some(original.as_str()));
    session.regenerate_id(true);
    assert_ne!(session.id(), original);
    assert_eq!(session.delete_id(), Some(original));
}

fn destroyed() -> Session {
    let session = Session::new();
    session.destroy();
    session
}

#[test]
#[should_panic(expected = "Session has been destroyed")]
fn a_destroyed_session_flash_throws() {
    destroyed().flash("hello", "world");
}

#[test]
#[should_panic(expected = "Session has been destroyed")]
fn a_destroyed_session_regenerate_id_throws() {
    destroyed().regenerate_id(false);
}

#[test]
#[should_panic(expected = "Session has been destroyed")]
fn a_destroyed_session_set_throws() {
    destroyed().set("hello", "world");
}

#[test]
#[should_panic(expected = "Session has been destroyed")]
fn a_destroyed_session_unset_throws() {
    destroyed().unset("hello");
}

#[test]
fn a_destroyed_session_get_returns_none() {
    let session = Session::new();
    session.set("hello", "world");
    assert_eq!(session.get::<String>("hello").as_deref(), Some("world"));
    session.destroy();
    assert_eq!(session.get_value("hello"), None);
}

#[test]
fn a_destroyed_session_has_returns_false() {
    let session = Session::new();
    session.set("hello", "world");
    assert!(session.has("hello"));
    session.destroy();
    assert!(!session.has("hello"));
}
