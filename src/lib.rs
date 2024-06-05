use std::collections::HashMap;
use std::num::NonZeroU16;

use anyhow::{Context as _, Result};
use bytes::{Bytes, BytesMut};
use header::{CONTENT_LENGTH, CONTENT_TYPE, TEXT_PLAIN};
use tokio::net::TcpStream;

use crate::header::HeaderMap;
use crate::io::{RequestReader, ResponseWriter};

pub(crate) mod header;
pub(crate) mod io;

#[derive(Clone, Debug, Default)]
#[repr(transparent)]
pub struct Body(Bytes);

impl std::ops::Deref for Body {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Bytes> for Body {
    #[inline]
    fn from(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl From<BytesMut> for Body {
    #[inline]
    fn from(bytes: BytesMut) -> Self {
        Self(bytes.freeze())
    }
}

impl From<&[u8]> for Body {
    #[inline]
    fn from(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            Self::default()
        } else {
            Self::from(Bytes::copy_from_slice(bytes))
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Request {
    // TODO: method as an enum
    method: Bytes,
    target: Bytes,
    version: Bytes,
    headers: HeaderMap,
    body: Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct StatusCode(NonZeroU16);

impl StatusCode {
    pub const OK: StatusCode = StatusCode(unsafe { NonZeroU16::new_unchecked(200) });
    pub const NOT_FOUND: StatusCode = StatusCode(unsafe { NonZeroU16::new_unchecked(404) });

    #[inline]
    pub fn as_u16(&self) -> u16 {
        self.0.into()
    }

    // TODO: see optimizations in hyper
    #[inline]
    pub fn as_str(&self) -> &str {
        match self.as_u16() {
            200 => "OK",
            404 => "Not Found",
            code => unimplemented!("string representation of {code:?}"),
        }
    }
}

impl Default for StatusCode {
    #[inline]
    fn default() -> Self {
        Self::OK
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub(crate) version: Bytes,
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Body,
}

impl Response {
    #[inline]
    pub fn from_request(request: &Request) -> ResponseBuilder {
        ResponseBuilder {
            version: request.version.clone(),
            status: StatusCode::default(),
            headers: HashMap::new(),
            body: BytesMut::new(),
        }
    }
}

#[derive(Debug)]
pub struct ResponseBuilder {
    version: Bytes,
    status: StatusCode,
    headers: HashMap<Bytes, Bytes>,
    body: BytesMut,
}

impl ResponseBuilder {
    #[inline]
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: Bytes, value: Bytes) -> Self {
        self.headers.insert(name, value);
        self
    }

    fn build_response(
        version: Bytes,
        status: StatusCode,
        mut headers: HashMap<Bytes, Bytes>,
        body: Body,
    ) -> Response {
        // insert/overwrite with the final content length
        let content_length = match body.len() {
            0 => Bytes::from_static(b"0"),
            len => len.to_string().into(),
        };
        headers.insert(CONTENT_LENGTH, content_length);

        Response {
            version,
            status,
            headers: HeaderMap::from_iter(headers),
            body,
        }
    }

    #[inline]
    pub fn text_plain(mut self, body: impl Into<Body>) -> Response {
        self = self.header(CONTENT_TYPE, TEXT_PLAIN);
        Self::build_response(self.version, self.status, self.headers, body.into())
    }

    #[inline]
    pub fn build(self) -> Response {
        Self::build_response(self.version, self.status, self.headers, self.body.into())
    }
}

/// Handle a HTTP/1.1 client connection
pub async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let (reader, writer) = stream.split();
    let mut reader = RequestReader::new(reader);
    let mut writer = ResponseWriter::new(writer);

    let req = reader.read_request().await.context("read request")?;

    println!("{req:?}");

    // TODO: extract to a router and magic handlers
    let resp = match req.target.as_ref() {
        b"/" => Response::from_request(&req).status(StatusCode::OK).build(),

        b"/user-agent" | b"/user-agent/" => req.headers.get(b"user-agent").map_or_else(
            || {
                Response::from_request(&req)
                    .status(StatusCode::NOT_FOUND)
                    .build()
            },
            |user_agent| {
                Response::from_request(&req)
                    .status(StatusCode::OK)
                    .text_plain(user_agent)
            },
        ),

        url if url.starts_with(b"/echo") => {
            let msg = url.strip_prefix(b"/echo/").unwrap_or_default();

            Response::from_request(&req)
                .status(StatusCode::OK)
                .text_plain(msg)
        }

        _ => Response::from_request(&req)
            .status(StatusCode::NOT_FOUND)
            .build(),
    };

    println!("{resp:?}");

    writer.write_response(resp).await.context("write response")
}
