//! Local files as lazy files, ported from Remix's `fs`.
//!
//! [open_lazy_file] reads a file's metadata now and its bytes only when streamed, one range
//! at a time, so a sliced file reads only its slice. [write_file] and [write_to] stream any
//! [Streamable] (a [LazyFile], [LazyBlob], in-memory [File] or [Body]) to disk.
//!
//! ```no_run
//! use rsmix_fs::{open_lazy_file, write_file, OpenLazyFileOptions};
//!
//! # async fn demo() -> std::io::Result<()> {
//! let file = open_lazy_file("public/logo.png", OpenLazyFileOptions::default())?;
//! assert_eq!(file.content_type(), "image/png");
//! write_file("backup/logo.png", &file).await?;
//! # Ok(())
//! # }
//! ```

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rsmix_fetch::{Body, File};
use rsmix_lazy_file::{LazyBlob, LazyContent, LazyFile, LazyFileOptions};

/// Overrides for [open_lazy_file].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenLazyFileOptions {
    /// The file's name. Default: the path as given.
    pub name: Option<String>,
    /// The MIME type. Default: from the extension (`""` when unknown).
    pub content_type: Option<String>,
    /// Milliseconds since the Unix epoch. Default: the file's modification time.
    pub last_modified: Option<u64>,
}

/// A file on disk, read when streamed.
struct FileContent {
    path: PathBuf,
    size: u64,
}

impl LazyContent for FileContent {
    fn byte_length(&self) -> u64 {
        self.size
    }

    fn stream(&self, start: u64, end: u64) -> Body {
        let open = || -> io::Result<fs::File> {
            let mut file = fs::File::open(&self.path)?;
            file.seek(SeekFrom::Start(start))?;
            Ok(file)
        };
        match open() {
            Ok(file) => Body::from_reader(file.take(end - start)),
            Err(error) => Body::from_task(move |tx| async move { tx.abort(error) }),
        }
    }
}

/// A [LazyFile] for the file at `path`: its size, type (from the extension) and modification
/// time are read now, its bytes when it is streamed. Fails like the filesystem does for a
/// missing file (`NotFound`), and with `Path "<path>" is not a file` for anything else.
pub fn open_lazy_file(path: impl AsRef<Path>, options: OpenLazyFileOptions) -> io::Result<LazyFile> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other(format!("Path \"{}\" is not a file", path.display())));
    }
    let name = options.name.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let content_type = options.content_type.unwrap_or_else(|| rsmix_mime::detect_mime_type(&path.to_string_lossy()).unwrap_or_default().to_string());
    let last_modified = options.last_modified.or_else(|| {
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        u64::try_from(modified.as_millis()).ok()
    });
    let content = FileContent { path: path.to_path_buf(), size: metadata.len() };
    Ok(LazyFile::from_content(content, name, LazyFileOptions { content_type, last_modified, ..Default::default() }))
}

/// Anything with a `stream()`: what [write_file] accepts.
pub trait Streamable {
    fn stream(&self) -> Body;
}

impl Streamable for LazyFile {
    fn stream(&self) -> Body {
        LazyFile::stream(self)
    }
}

impl Streamable for LazyBlob {
    fn stream(&self) -> Body {
        LazyBlob::stream(self)
    }
}

impl Streamable for File {
    fn stream(&self) -> Body {
        Body::from(self.bytes.clone())
    }
}

/// Write `file` to `path` (created or truncated), streaming it chunk by chunk. The parent
/// directory must exist. A failure of the source stream fails the write with its message.
pub async fn write_file(path: impl AsRef<Path>, file: &impl Streamable) -> io::Result<()> {
    write_to(fs::File::create(path)?, file).await
}

/// Write `file` to an open file or any other writer (Remix's file descriptor and `FileHandle`
/// forms of `writeFile`), then flush it.
pub async fn write_to(mut writer: impl Write, file: &impl Streamable) -> io::Result<()> {
    let mut body = file.stream();
    while let Some(chunk) = body.chunk().await {
        writer.write_all(&chunk.map_err(io::Error::other)?)?;
    }
    writer.flush()
}
