//! Filesystem session storage.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::{Error, Session, SessionData, SessionStorage};

/// Keeps each session in a JSON file under a directory dedicated to this storage, named by
/// the SHA-256 of its id (`ab/cdef...`), so ids never become paths.
#[derive(Clone, Debug)]
pub struct FsStorage {
    root: PathBuf,
    use_unknown_ids: bool,
}

impl FsStorage {
    /// Creates the directory if needed; fails if the path exists and is not a directory.
    pub fn new(dir: impl AsRef<Path>) -> std::io::Result<FsStorage> {
        let root = std::path::absolute(dir.as_ref())?;
        match std::fs::metadata(&root) {
            Ok(m) if !m.is_dir() => {
                return Err(std::io::Error::other(format!("{} is not a directory", root.display())));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => std::fs::create_dir_all(&root)?,
            Err(e) => return Err(e),
        }
        Ok(FsStorage { root, use_unknown_ids: false })
    }

    /// Reuse ids sent by the client that have no file (off by default).
    pub fn use_unknown_ids(mut self, on: bool) -> FsStorage {
        self.use_unknown_ids = on;
        self
    }

    fn file(&self, id: &str) -> PathBuf {
        let hash = rmx_crypto::hex(&rmx_crypto::sha256(id.as_bytes()));
        self.root.join(&hash[..2]).join(&hash[2..])
    }

    async fn delete(&self, id: &str) -> Result<(), Error> {
        match tokio::fs::remove_file(self.file(id)).await {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

impl SessionStorage for FsStorage {
    async fn read(&self, cookie: Option<&str>) -> Result<Session, Error> {
        let Some(id) = cookie.filter(|id| !id.is_empty()) else {
            return Ok(Session::new());
        };
        match tokio::fs::read(self.file(id)).await {
            Ok(bytes) => Ok(Session::with_data(id, serde_json::from_slice::<SessionData>(&bytes)?)),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Ok(if self.use_unknown_ids { Session::with_data(id, SessionData::default()) } else { Session::new() })
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn save(&self, session: &Session) -> Result<Option<String>, Error> {
        if let Some(old) = session.delete_id() {
            self.delete(&old).await?;
        }
        let id = session.id();
        if session.is_destroyed() {
            self.delete(&id).await?;
            return Ok(Some(String::new()));
        }
        if !session.is_dirty() {
            return Ok(None);
        }
        let file = self.file(&id);
        if let Some(dir) = file.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        tokio::fs::write(&file, serde_json::to_vec(&session.data())?).await?;
        Ok(Some(id))
    }
}
