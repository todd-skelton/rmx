//! Sessions (Remix's `session` package): data that persists for one user across requests.
//!
//! A [`Session`] holds values and flash messages. A [`SessionStorage`] turns the value of
//! the session cookie into a session and back: [`CookieStorage`] keeps everything in the
//! cookie, [`MemoryStorage`] and [`FsStorage`] keep data on the server and only the id in
//! the cookie. The `session` middleware in `rsmix-session-middleware` reads and writes the cookie.
//!
//! ```
//! use rsmix_session::Session;
//!
//! let session = Session::new();
//! session.set("user_id", 42);
//! session.flash("message", "Welcome back");
//! assert_eq!(session.get::<i64>("user_id"), Some(42));
//! assert!(session.is_dirty());
//! ```

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(feature = "fs")]
mod fs;
#[cfg(feature = "fs")]
pub use fs::FsStorage;

/// Errors from a storage backend.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Session values and the flash values for the next request, stored as the JSON array
/// `[values, flash]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionData(pub Map<String, Value>, pub Map<String, Value>);

/// A new random session id (a v4 UUID).
pub fn create_session_id() -> String {
    rsmix_crypto::random_uuid()
}

/// One user's session. Cloning shares the same session, so middleware and handlers see
/// the same data; all methods take `&self`.
///
/// Values are JSON ([`serde_json::Value`]); [`Session::get`] deserializes into any type.
/// Setting a value on a destroyed session panics, like Remix throws.
#[derive(Clone)]
pub struct Session(Arc<Mutex<State>>);

struct State {
    original_id: String,
    id: String,
    delete_id: Option<String>,
    values: Map<String, Value>,
    flash: Map<String, Value>,
    next: Map<String, Value>,
    destroyed: bool,
    dirty: bool,
}

impl Default for Session {
    fn default() -> Session {
        Session::new()
    }
}

impl Session {
    /// An empty session with a new id.
    pub fn new() -> Session {
        Session::with_data(create_session_id(), SessionData::default())
    }

    /// A session loaded from storage. Flash values in `data` are readable during this
    /// request and gone after the next save, so the session starts dirty when it has any.
    pub fn with_data(id: impl Into<String>, data: SessionData) -> Session {
        let id = id.into();
        let SessionData(values, flash) = data;
        Session(Arc::new(Mutex::new(State {
            original_id: id.clone(),
            id,
            delete_id: None,
            dirty: !flash.is_empty(),
            values,
            flash,
            next: Map::new(),
            destroyed: false,
        })))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn live(&self) -> MutexGuard<'_, State> {
        let state = self.state();
        assert!(!state.destroyed, "Session has been destroyed");
        state
    }

    /// The current id (changes after [`Session::regenerate_id`]).
    pub fn id(&self) -> String {
        self.state().id.clone()
    }

    /// What storage should write: the values and the flash values set during this request.
    pub fn data(&self) -> SessionData {
        let s = self.state();
        if s.destroyed {
            return SessionData::default();
        }
        SessionData(s.values.clone(), s.next.clone())
    }

    /// The id whose stored data should be deleted on save (set by `regenerate_id(true)`).
    pub fn delete_id(&self) -> Option<String> {
        self.state().delete_id.clone()
    }

    /// Mark the session destroyed: reads return nothing and storage deletes it on save.
    pub fn destroy(&self) {
        self.state().destroyed = true;
    }

    pub fn is_destroyed(&self) -> bool {
        self.state().destroyed
    }

    /// Whether anything changed, so storage needs to save it.
    pub fn is_dirty(&self) -> bool {
        self.state().dirty
    }

    /// A value readable only during the next request.
    pub fn flash(&self, key: impl Into<String>, value: impl Serialize) {
        let mut s = self.live();
        s.next.insert(key.into(), to_value(value));
        s.dirty = true;
    }

    /// A value (or a flash value from the previous request), deserialized as `T`. `None` when
    /// missing or not a `T`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.get_value(key).and_then(|v| serde_json::from_value(v).ok())
    }

    /// A value as raw JSON.
    pub fn get_value(&self, key: &str) -> Option<Value> {
        let s = self.state();
        if s.destroyed {
            return None;
        }
        s.values.get(key).or_else(|| s.flash.get(key)).cloned()
    }

    pub fn has(&self, key: &str) -> bool {
        let s = self.state();
        !s.destroyed && (s.values.contains_key(key) || s.flash.contains_key(key))
    }

    /// Give the session a new id, keeping its data. Call after login or any privilege
    /// change. With `delete_old`, storage deletes the data under the old id on save.
    pub fn regenerate_id(&self, delete_old: bool) {
        let mut s = self.live();
        if delete_old {
            s.delete_id = Some(s.original_id.clone());
        }
        s.id = create_session_id();
        s.dirty = true;
    }

    /// Set a value. Setting `null` (for example `None`) removes the key.
    pub fn set(&self, key: impl Into<String>, value: impl Serialize) {
        let mut s = self.live();
        match to_value(value) {
            Value::Null => {
                s.values.remove(&key.into());
            }
            v => {
                s.values.insert(key.into(), v);
            }
        }
        s.dirty = true;
    }

    /// The number of values and flash values.
    pub fn len(&self) -> usize {
        let s = self.state();
        if s.destroyed {
            0
        } else {
            s.values.len() + s.flash.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn unset(&self, key: &str) {
        let mut s = self.live();
        s.values.remove(key);
        s.dirty = true;
    }

    /// Whether both handles share one session.
    pub fn ptr_eq(&self, other: &Session) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.state();
        f.debug_struct("Session")
            .field("id", &s.id)
            .field("values", &s.values)
            .field("flash", &s.flash)
            .field("destroyed", &s.destroyed)
            .field("dirty", &s.dirty)
            .finish()
    }
}

fn to_value(value: impl Serialize) -> Value {
    serde_json::to_value(value).expect("session values must serialize to JSON")
}

/// Turns session cookie values into sessions and back.
pub trait SessionStorage: Send + Sync + 'static {
    /// The session for a cookie value (`None` when the request has no session cookie).
    fn read(&self, cookie: Option<&str>) -> impl Future<Output = Result<Session, Error>> + Send;

    /// Persist the session and return the new cookie value: `None` when the cookie should
    /// not change, `Some("")` when the session was destroyed.
    fn save(&self, session: &Session) -> impl Future<Output = Result<Option<String>, Error>> + Send;
}

/// Keeps all session data in the cookie itself (about 4 KB at most). Ids regenerated with
/// `delete_old` cannot be revoked, since there is no server-side copy.
#[derive(Clone, Copy, Debug, Default)]
pub struct CookieStorage;

impl CookieStorage {
    pub fn new() -> CookieStorage {
        CookieStorage
    }
}

#[derive(Serialize, Deserialize)]
struct CookiePayload {
    i: String,
    d: SessionData,
}

impl SessionStorage for CookieStorage {
    async fn read(&self, cookie: Option<&str>) -> Result<Session, Error> {
        Ok(cookie
            .and_then(|c| serde_json::from_str::<CookiePayload>(c).ok())
            .map(|p| Session::with_data(p.i, p.d))
            .unwrap_or_default())
    }

    async fn save(&self, session: &Session) -> Result<Option<String>, Error> {
        if let Some(old) = session.delete_id() {
            eprintln!("Session ID {old} was regenerated, but the old session cannot be deleted when using cookie storage");
        }
        if session.is_destroyed() {
            return Ok(Some(String::new()));
        }
        if !session.is_dirty() {
            return Ok(None);
        }
        Ok(Some(serde_json::to_string(&CookiePayload { i: session.id(), d: session.data() })?))
    }
}

/// Keeps session data in process memory (lost on restart); good for development and tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryStorage {
    map: Arc<Mutex<std::collections::HashMap<String, SessionData>>>,
    use_unknown_ids: bool,
}

impl MemoryStorage {
    pub fn new() -> MemoryStorage {
        MemoryStorage::default()
    }

    /// Reuse ids sent by the client that are not in storage (off by default).
    pub fn use_unknown_ids(mut self, on: bool) -> MemoryStorage {
        self.use_unknown_ids = on;
        self
    }

    fn map(&self) -> MutexGuard<'_, std::collections::HashMap<String, SessionData>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionStorage for MemoryStorage {
    async fn read(&self, cookie: Option<&str>) -> Result<Session, Error> {
        let Some(id) = cookie.filter(|id| !id.is_empty()) else {
            return Ok(Session::new());
        };
        if let Some(data) = self.map().get(id) {
            return Ok(Session::with_data(id, data.clone()));
        }
        Ok(if self.use_unknown_ids { Session::with_data(id, SessionData::default()) } else { Session::new() })
    }

    async fn save(&self, session: &Session) -> Result<Option<String>, Error> {
        let mut map = self.map();
        if let Some(old) = session.delete_id() {
            map.remove(&old);
        }
        if session.is_destroyed() {
            map.remove(&session.id());
            return Ok(Some(String::new()));
        }
        if session.is_dirty() {
            map.insert(session.id(), session.data());
            return Ok(Some(session.id()));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
