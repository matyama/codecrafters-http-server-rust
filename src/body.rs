use std::ffi::OsStr;
use std::fs::Metadata;
use std::path::PathBuf;

use bytes::{Bytes, BytesMut};
use tokio::fs::File;
use tokio::io::AsyncRead;

use crate::header::ContentLength;

#[derive(Debug)]
pub struct FileBody {
    path: PathBuf,
    file: File,
    meta: Metadata,
}

impl FileBody {
    // NOTE: files are already buffered
    #[inline]
    pub fn into_reader(self) -> impl AsyncRead + Unpin {
        self.file
    }

    #[inline]
    pub(crate) fn as_path(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

#[derive(Debug)]
pub enum Body {
    Bytes(Bytes),
    File(FileBody),
}

impl Body {
    #[inline]
    pub fn empty() -> Self {
        Self::Bytes(Bytes::default())
    }

    #[inline]
    pub fn bytes(bytes: impl Into<Bytes>) -> Self {
        Self::Bytes(bytes.into())
    }

    pub async fn file(path: PathBuf, file: File) -> std::io::Result<Self> {
        let meta = file.metadata().await?;
        Ok(Self::from(FileBody { path, file, meta }))
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn len(&self) -> u64 {
        match self {
            Body::Bytes(bytes) => bytes.len() as u64,
            Body::File(file) => file.meta.len(),
        }
    }

    #[inline]
    pub fn content_length(&self) -> ContentLength {
        ContentLength::from(self.len())
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<BytesMut> for Body {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self::Bytes(bytes.freeze())
    }
}

impl From<&[u8]> for Body {
    #[inline]
    fn from(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            Self::empty()
        } else {
            Self::from(Bytes::copy_from_slice(bytes))
        }
    }
}

impl From<FileBody> for Body {
    #[inline]
    fn from(file: FileBody) -> Self {
        Self::File(file)
    }
}
