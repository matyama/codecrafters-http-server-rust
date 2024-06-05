use std::{str::FromStr, sync::Arc};

use bytes::{Bytes, BytesMut};

use crate::Encoding;

pub const ACCEPT_ENCODING: Bytes = Bytes::from_static(b"Accept-Encoding");

pub const CONTENT_TYPE: Bytes = Bytes::from_static(b"Content-Type");
pub const CONTENT_LENGTH: Bytes = Bytes::from_static(b"Content-Length");
pub const CONTENT_ENCODING: Bytes = Bytes::from_static(b"Content-Encoding");

// TODO: enum MimeType: Into<Bytes> + FromStr
pub const TEXT_PLAIN: Bytes = Bytes::from_static(b"text/plain");
pub const OCTET_STREAM: Bytes = Bytes::from_static(b"application/octet-stream");

pub trait ToHeaderName {
    fn header_name() -> Bytes;
}

#[derive(Debug, Default)]
#[repr(transparent)]
pub struct AcceptEncoding(Vec<Encoding>);

impl From<Bytes> for AcceptEncoding {
    fn from(value: Bytes) -> Self {
        let mut value = value.clone();

        let mut encs = Vec::with_capacity(8);

        while let Some(pos) = value.iter().position(|&b| b == b',') {
            let enc = value.split_to(pos);
            let _ = value.split_to(1);

            // skip over whitespace
            if let Some(at) = value.iter().position(|&b| !b.is_ascii_whitespace()) {
                let _ = value.split_to(at);
            }

            if let Ok(enc) = Encoding::try_from(enc) {
                encs.push(enc);
            }
        }

        if let Ok(enc) = Encoding::try_from(value.as_ref()) {
            encs.push(enc);
        }

        Self(encs)
    }
}

impl ToHeaderName for AcceptEncoding {
    #[inline]
    fn header_name() -> Bytes {
        ACCEPT_ENCODING
    }
}

impl From<AcceptEncoding> for Option<Bytes> {
    fn from(encoding: AcceptEncoding) -> Self {
        const SEP: Bytes = Bytes::from_static(b", ");

        if encoding.0.is_empty() {
            return None;
        }

        let mut bytes = BytesMut::with_capacity(32);

        let encs = encoding.0.into_iter().map(Bytes::from);
        for enc in itertools::intersperse(encs, SEP) {
            bytes.extend_from_slice(&enc);
        }

        Some(bytes.freeze())
    }
}

// TODO: ideally some persistent map (immutable, with structural sharing)
#[derive(Clone, Debug)]
#[repr(transparent)]
pub struct HeaderMap(Arc<[(Bytes, Bytes)]>);

impl HeaderMap {
    #[inline]
    pub fn from_iter(iter: impl IntoIterator<Item = (Bytes, Bytes)>) -> Self {
        Self(iter.into_iter().collect())
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (Bytes, Bytes)> + '_ {
        self.0
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<Bytes> {
        let key = key.as_ref();
        self.0.iter().find_map(|(name, value)| {
            if name.matches(key) {
                Some(value.clone())
            } else {
                None
            }
        })
    }

    #[inline]
    pub fn extract<V>(&self) -> Option<V>
    where
        V: ToHeaderName,
        Bytes: Into<V>,
    {
        self.get(V::header_name()).map(Into::<V>::into)
    }

    pub fn read<K, V>(&self, key: K) -> Option<V>
    where
        K: AsRef<[u8]>,
        V: FromStr,
    {
        let value = self.get(key)?;
        std::str::from_utf8(&value)
            .ok()
            .and_then(|value| value.parse().ok())
    }

    #[inline]
    pub(crate) fn builder() -> HeaderMapBuilder {
        HeaderMapBuilder::default()
    }
}

#[derive(Debug, Default)]
#[repr(transparent)]
pub struct HeaderMapBuilder(Vec<(Bytes, Bytes)>);

impl HeaderMapBuilder {
    // TODO: handle duplicate headers
    #[inline]
    pub fn insert(&mut self, name: Bytes, value: impl Into<Bytes>) {
        self.0.push((name, value.into()))
    }

    #[inline]
    pub fn build(self) -> HeaderMap {
        HeaderMap(Arc::from(self.0.into_boxed_slice()))
    }
}

const CASE_SHIFT: u8 = b'a'.abs_diff(b'A');

pub(crate) fn compare<F, D, T>(cmp: F, data: D, target: T) -> bool
where
    F: Fn(u8, u8) -> bool,
    D: AsRef<[u8]>,
    T: AsRef<[u8]>,
{
    let data = data.as_ref();
    let target = target.as_ref();

    if data.len() != target.len() {
        return false;
    }

    data.iter().zip(target.iter()).all(|(&x, &y)| cmp(x, y))
}

fn ignore_case_eq(x: u8, y: u8) -> bool {
    x == y || (x.is_ascii_alphabetic() && y.is_ascii_alphabetic() && x.abs_diff(y) == CASE_SHIFT)
}

trait BytesExt {
    /// Returns `true` iff `self` matches given target ignoring casing of ASCII (alpha) characters
    fn matches(&self, target: impl AsRef<[u8]>) -> bool;
}

impl<'a> BytesExt for &'a [u8] {
    #[inline]
    fn matches(&self, target: impl AsRef<[u8]>) -> bool {
        compare(ignore_case_eq, self, target)
    }
}

impl BytesExt for Bytes {
    #[inline]
    fn matches(&self, target: impl AsRef<[u8]>) -> bool {
        compare(ignore_case_eq, self, target)
    }
}
