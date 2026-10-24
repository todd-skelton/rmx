//! The request scenarios Remix's storage tests share (`requestIndex`, `requestFlash`, ...).

#![allow(dead_code)]

use rsmix_session::{Session, SessionStorage};

pub struct Step {
    pub cookie: Option<String>,
    pub session: Session,
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

fn request(storage: &impl SessionStorage, cookie: Option<&str>, act: impl FnOnce(&Session)) -> Step {
    run(async {
        let session = storage.read(cookie).await.unwrap();
        act(&session);
        Step { cookie: storage.save(&session).await.unwrap(), session }
    })
}

pub fn read(storage: &impl SessionStorage, cookie: Option<&str>) -> Session {
    run(storage.read(cookie)).unwrap()
}

/// Remix's `requestIndex` that counts visits.
pub fn count(storage: &impl SessionStorage, cookie: Option<&str>) -> Step {
    request(storage, cookie, |s| {
        let n = s.get::<i64>("count").unwrap_or(0);
        s.set("count", n + 1);
    })
}

pub fn index(storage: &impl SessionStorage, cookie: Option<&str>) -> Step {
    request(storage, cookie, |_| {})
}

pub fn destroy(storage: &impl SessionStorage, cookie: Option<&str>) -> Step {
    request(storage, cookie, Session::destroy)
}

pub fn flash(storage: &impl SessionStorage, cookie: Option<&str>) -> Step {
    request(storage, cookie, |s| s.flash("message", "success!"))
}

pub fn login(storage: &impl SessionStorage, cookie: Option<&str>, delete_old: bool) -> Step {
    request(storage, cookie, |s| s.regenerate_id(delete_old))
}

fn get_count(step: &Step) -> Option<i64> {
    step.session.get::<i64>("count")
}

pub fn persists_session_data_across_requests(storage: &impl SessionStorage) {
    let r1 = count(storage, None);
    assert_eq!(get_count(&r1), Some(1));
    let r2 = count(storage, r1.cookie.as_deref());
    assert_eq!(get_count(&r2), Some(2));
    let r3 = count(storage, r2.cookie.as_deref());
    assert_eq!(get_count(&r3), Some(3));
}

pub fn clears_session_data_when_the_session_is_destroyed(storage: &impl SessionStorage) {
    let r1 = count(storage, None);
    let r2 = count(storage, r1.cookie.as_deref());
    assert_eq!(get_count(&r2), Some(2));
    let r3 = destroy(storage, r2.cookie.as_deref());
    assert!(r3.session.is_destroyed());
    let r4 = count(storage, r3.cookie.as_deref());
    assert_eq!(get_count(&r4), Some(1));
}

pub fn does_not_set_a_cookie_when_session_data_is_not_changed(storage: &impl SessionStorage) {
    let r = index(storage, None);
    assert!(!r.session.is_dirty());
    assert_eq!(r.cookie, None);
}

pub fn makes_flash_data_available_only_on_the_next_request(storage: &impl SessionStorage) {
    let message = |step: &Step| step.session.get::<String>("message");
    let r1 = index(storage, None);
    assert_eq!(message(&r1), None);
    let r2 = flash(storage, r1.cookie.as_deref());
    assert_eq!(message(&r2), None);
    let r3 = index(storage, r2.cookie.as_deref());
    assert_eq!(message(&r3).as_deref(), Some("success!"));
    let r4 = index(storage, r3.cookie.as_deref());
    assert_eq!(message(&r4), None);
}

pub fn leaves_old_session_data_in_storage_by_default_when_the_id_is_regenerated(storage: &impl SessionStorage) {
    let r1 = count(storage, None);
    assert_eq!(get_count(&r1), Some(1));
    let r2 = login(storage, r1.cookie.as_deref(), false);
    assert_ne!(r2.session.id(), r1.session.id());
    let r3 = count(storage, r2.cookie.as_deref());
    assert_eq!(get_count(&r3), Some(2));
    let r4 = count(storage, r1.cookie.as_deref());
    assert_eq!(get_count(&r4), Some(2), "old session data should still be in storage");
}

pub fn deletes_old_session_data_when_the_id_is_regenerated_with_delete_old(storage: &impl SessionStorage) {
    let r1 = count(storage, None);
    let r2 = login(storage, r1.cookie.as_deref(), true);
    assert_ne!(r2.session.id(), r1.session.id());
    let r3 = count(storage, r2.cookie.as_deref());
    assert_eq!(get_count(&r3), Some(2));
    let r4 = count(storage, r1.cookie.as_deref());
    assert_eq!(get_count(&r4), Some(1), "old session data should be deleted");
}
