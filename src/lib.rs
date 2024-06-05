use std::num::NonZeroU16;

use anyhow::{Context as _, Result};
use bytes::{Bytes, BytesMut};
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
    // TODO: include headers with at least to Content-Length of the body
    // headers: HeaderMap,
    pub(crate) body: Body,
}

impl Response {
    #[inline]
    pub fn from_request(request: &Request) -> ResponseBuilder {
        ResponseBuilder {
            version: request.version.clone(),
            status: StatusCode::default(),
            body: BytesMut::new(),
        }
    }
}

#[derive(Debug)]
pub struct ResponseBuilder {
    version: Bytes,
    status: StatusCode,
    body: BytesMut,
}

impl ResponseBuilder {
    #[inline]
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    #[inline]
    pub fn body(self, body: impl Into<Body>) -> Response {
        Response {
            version: self.version,
            status: self.status,
            body: body.into(),
        }
    }

    #[inline]
    pub fn build(self) -> Response {
        Response {
            version: self.version,
            status: self.status,
            body: self.body.into(),
        }
    }
}

/// Handle a HTTP/1.1 client connection
pub async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let (reader, writer) = stream.split();
    let mut reader = RequestReader::new(reader);
    let mut writer = ResponseWriter::new(writer);

    let req = reader.read_request().await.context("read request")?;

    println!("{req:?}");

    // TODO: extract to router
    let resp = match req.target.as_ref() {
        b"/" => Response::from_request(&req).status(StatusCode::OK).build(),
        _ => Response::from_request(&req)
            .status(StatusCode::NOT_FOUND)
            .build(),
    };

    println!("{resp:?}");

    writer.write_response(resp).await.context("write response")
}
