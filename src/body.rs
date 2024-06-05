use std::fs::Metadata;

use bytes::{Bytes, BytesMut};
use tokio::fs::File;
use tokio::io::{AsyncBufRead, BufReader};

#[derive(Debug)]
pub struct FileBody {
    file: File,
    meta: Metadata,
}

impl FileBody {
    #[inline]
    pub fn into_reader(self) -> impl AsyncBufRead + Unpin {
        BufReader::new(self.file)
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

    pub async fn file(file: File) -> std::io::Result<Self> {
        let meta = file.metadata().await?;
        Ok(Self::from(FileBody { file, meta }))
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.content_length() == 0
    }

    #[inline]
    pub fn content_length(&self) -> u64 {
        match self {
            Body::Bytes(bytes) => bytes.len() as u64,
            Body::File(file) => file.meta.len(),
        }
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
