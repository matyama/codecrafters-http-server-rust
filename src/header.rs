use std::{str::FromStr, sync::Arc};

use bytes::Bytes;

pub const CONTENT_LENGTH: Bytes = Bytes::from_static(b"Content-Length");

// TODO: ideally some persistent map (immutable, with structural sharing)
#[derive(Clone, Debug)]
#[repr(transparent)]
pub(crate) struct HeaderMap(Arc<[(Bytes, Bytes)]>);

impl HeaderMap {
    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<Bytes> {
        let key = key.as_ref();
        self.0.iter().find_map(|(name, value)| {
            if name.as_ref() == key {
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
pub(crate) struct HeaderMapBuilder(Vec<(Bytes, Bytes)>);

impl HeaderMapBuilder {
    // TODO: handle duplicate headers
    #[inline]
    pub fn insert(&mut self, name: Bytes, value: Bytes) {
        self.0.push((name, value))
    }

    #[inline]
    pub fn build(self) -> HeaderMap {
        HeaderMap(Arc::from(self.0.into_boxed_slice()))
    }
}
