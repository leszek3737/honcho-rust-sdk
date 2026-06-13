//! File source abstraction for uploads.

use std::path::{Path, PathBuf};
use std::pin::Pin;

use tokio::io::AsyncRead;

/// A source for file data that will be uploaded.
///
/// Construct with [`FileSource::bytes`], [`FileSource::path`], or
/// [`FileSource::stream`], or convert from [`PathBuf`]/[`std::path::Path`] via the
/// `From` impls.
///
/// The API currently accepts `text/plain`, `application/pdf`, and
/// `application/json`; other MIME types may be rejected by the server.
pub enum FileSource {
    /// Raw bytes with explicit filename and content type.
    Bytes {
        /// File name to send.
        filename: String,
        /// Raw file data. Cloned by reference-count, not deep-copied, on retry.
        bytes: bytes::Bytes,
        /// MIME content type.
        content_type: String,
    },
    /// A filesystem path. Resolved at upload time.
    Path(PathBuf),
    /// A streaming reader — fully buffered into memory before uploading.
    Stream {
        /// File name to send.
        filename: String,
        /// Async reader producing the file data.
        reader: Pin<Box<dyn AsyncRead + Send>>,
        /// MIME content type.
        content_type: String,
    },
}

impl std::fmt::Debug for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes {
                filename,
                bytes,
                content_type,
            } => f
                .debug_struct("Bytes")
                .field("filename", filename)
                .field("bytes", &bytes.len())
                .field("content_type", content_type)
                .finish(),
            Self::Path(p) => f.debug_tuple("Path").field(p).finish(),
            Self::Stream {
                filename,
                content_type,
                ..
            } => f
                .debug_struct("Stream")
                .field("filename", filename)
                .field("content_type", content_type)
                .finish_non_exhaustive(),
        }
    }
}

impl FileSource {
    /// Create a `Bytes` variant from explicit parts.
    ///
    /// `data` accepts anything convertible into `Vec<u8>` (e.g. `Vec<u8>`,
    /// `&[u8]`, `&str`); it is stored as [`bytes::Bytes`] internally so that
    /// upload retries clone by reference-count rather than deep-copying.
    ///
    /// The content type is validated when the upload is sent (during multipart
    /// form construction), not here, to keep this constructor infallible.
    ///
    /// The API currently accepts `text/plain`, `application/pdf`, and
    /// `application/json`; other MIME types may be rejected by the server.
    pub fn bytes(
        filename: impl Into<String>,
        data: impl Into<Vec<u8>>,
        content_type: impl Into<String>,
    ) -> Self {
        Self::Bytes {
            filename: filename.into(),
            bytes: bytes::Bytes::from(data.into()),
            content_type: content_type.into(),
        }
    }

    /// Create a `Path` variant.
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    /// Create a `Stream` variant from an [`AsyncRead`] source.
    ///
    /// The reader is fully consumed with [`tokio::io::AsyncReadExt::read_to_end`]
    /// and buffered into memory before the upload begins. This is **not** true
    /// streaming — the entire payload resides in memory during the request.
    ///
    /// For files on disk, prefer [`FileSource::path`].
    ///
    /// The API currently accepts `text/plain`, `application/pdf`, and
    /// `application/json`; other MIME types may be rejected by the server.
    ///
    /// # Examples
    ///
    /// ```
    /// use honcho_ai::FileSource;
    ///
    /// let cursor = std::io::Cursor::new(b"hello".to_vec());
    /// let src = FileSource::stream("out.txt", cursor, "text/plain");
    /// ```
    pub fn stream(
        filename: impl Into<String>,
        reader: impl AsyncRead + Send + 'static,
        content_type: impl Into<String>,
    ) -> Self {
        Self::Stream {
            filename: filename.into(),
            reader: Box::pin(reader),
            content_type: content_type.into(),
        }
    }
}

impl From<PathBuf> for FileSource {
    fn from(p: PathBuf) -> Self {
        Self::Path(p)
    }
}

impl From<&Path> for FileSource {
    fn from(p: &Path) -> Self {
        Self::Path(p.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_impl_all;

    use super::*;

    // `FileSource` must stay `Send` so uploads can cross `.await` points and be
    // driven from multi-threaded runtimes. Behavioural coverage of the upload
    // path lives in `session.rs` tests, which exercise the production
    // `Session::upload_file(...).send()` flow end-to-end.
    assert_impl_all!(FileSource: Send);
}
