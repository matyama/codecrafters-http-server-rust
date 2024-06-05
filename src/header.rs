use std::{str::FromStr, sync::Arc};

use bytes::Bytes;

pub const CONTENT_TYPE: Bytes = Bytes::from_static(b"Content-Type");
pub const CONTENT_LENGTH: Bytes = Bytes::from_static(b"Content-Length");

// TODO: enum MimeType: Into<Bytes> + FromStr
pub const TEXT_PLAIN: Bytes = Bytes::from_static(b"text/plain");
pub const OCTET_STREAM: Bytes = Bytes::from_static(b"application/octet-stream");

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
