use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};

use crate::encoding::{Encoding, SystemEncoder};

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

pub trait IntoHeaderValue {
    fn into_header_value(self) -> Bytes;
}

#[derive(Debug, Default)]
#[repr(transparent)]
pub struct AcceptEncoding(Vec<Encoding>);

impl AcceptEncoding {
    #[inline]
    pub(crate) fn select(&self, supported: &HashSet<Encoding>) -> Option<Encoding> {
        self.0.iter().find(|enc| supported.contains(enc)).copied()
    }
}

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

#[derive(Debug)]
#[repr(transparent)]
pub struct ContentEncoding(Encoding);

impl std::fmt::Display for ContentEncoding {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ToHeaderName for ContentEncoding {
    #[inline]
    fn header_name() -> Bytes {
        CONTENT_ENCODING
    }
}

impl IntoHeaderValue for ContentEncoding {
    #[inline]
    fn into_header_value(self) -> Bytes {
        self.0.into()
    }
}

impl TryFrom<Bytes> for ContentEncoding {
    type Error = anyhow::Error;

    #[inline]
    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
        Encoding::try_from(bytes).map(Self)
    }
}

impl SystemEncoder for ContentEncoding {
    #[inline]
    fn program(&self) -> Option<&str> {
        self.0.program()
    }

    #[inline]
    fn command(&self) -> Option<tokio::process::Command> {
        self.0.command()
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct ContentLength(Bytes);

impl ToHeaderName for ContentLength {
    #[inline]
    fn header_name() -> Bytes {
        CONTENT_LENGTH
    }
}

impl IntoHeaderValue for ContentLength {
    #[inline]
    fn into_header_value(self) -> Bytes {
        self.0
    }
}

impl From<Bytes> for ContentLength {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl From<u64> for ContentLength {
    #[inline]
    fn from(len: u64) -> Self {
        Self(match len {
            0 => Bytes::from_static(b"0"),
            len => len.to_string().into(),
        })
    }
}

impl From<ContentLength> for Bytes {
    #[inline]
    fn from(ContentLength(len): ContentLength) -> Self {
        len
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
        Bytes: TryInto<V>,
    {
        self.get(V::header_name()).and_then(|v| v.try_into().ok())
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
    pub fn insert<H: ToHeaderName + IntoHeaderValue>(&self, header: H) -> Self {
        self.assoc(H::header_name(), header.into_header_value())
    }

    // NOTE: here we'd really benefit from a persistent data structure with structural sharing
    pub fn assoc<K, V>(&self, key: K, val: V) -> Self
    where
        K: Into<Bytes>,
        V: Into<Bytes>,
    {
        let key = key.into();
        let val = val.into();

        Self::from_iter(self.iter().map(|(k, v)| {
            if k.matches(&key) {
                (key.clone(), val.clone())
            } else {
                (k, v)
            }
        }))
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
    pub fn assoc(&mut self, name: Bytes, value: impl Into<Bytes>) {
        self.0.push((name, value.into()))
    }

    pub fn insert<H: ToHeaderName + IntoHeaderValue>(&mut self, header: H) {
        self.assoc(H::header_name(), header.into_header_value())
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
