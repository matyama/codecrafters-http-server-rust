use std::collections::HashMap;
use std::io::ErrorKind;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use bytes::{Bytes, BytesMut};
use header::{CONTENT_LENGTH, CONTENT_TYPE, TEXT_PLAIN};
use tokio::fs;
use tokio::net::TcpStream;

use crate::body::Body;
use crate::header::{AcceptEncoding, HeaderMap, CONTENT_ENCODING, OCTET_STREAM};
use crate::io::{FileWriter, RequestReader, ResponseWriter};

pub use config::Config;

pub(crate) mod body;
pub(crate) mod config;
pub(crate) mod header;
pub(crate) mod io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Gzip,
    Compress,
    Deflate,
    Br,
    Zstd,
}

macro_rules! encoding {
    ($($var:ident($name:ident, $enc:literal)),+) => {
        impl Encoding {
            $(pub const $name: Bytes = Bytes::from_static($enc);)+
        }

        impl TryFrom<&[u8]> for Encoding {
            type Error = anyhow::Error;

            #[inline]
            fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
                match bytes {
                    $($enc => Ok(Self::$var),)+
                    other => bail!("unsupported encoding '{}'", String::from_utf8_lossy(other)),
                }
            }
        }

        impl From<Encoding> for Bytes {
            #[inline]
            fn from(encoding: Encoding) -> Self {
                match encoding {
                    $(Encoding::$var => Encoding::$name,)+
                }
            }
        }
    };
}

impl TryFrom<Bytes> for Encoding {
    type Error = anyhow::Error;

    #[inline]
    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
        Self::try_from(bytes.as_ref())
    }
}

// TODO: support * and quality values (e.g., `*;q=0.1`)
// https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept-Encoding
encoding! {
    Gzip(GZIP, b"gzip"),
    Compress(COMPRESS, b"compress"),
    Deflate(DEFLATE, b"deflate"),
    Br(BR, b"br"),
    Zstd(ZSTD, b"zstd")
}

// TODO: other methods
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl TryFrom<Bytes> for Method {
    type Error = anyhow::Error;

    #[inline]
    fn try_from(method: Bytes) -> Result<Self, Self::Error> {
        match method.as_ref() {
            b"GET" => Ok(Self::Get),
            b"POST" => Ok(Self::Post),
            _ => bail!("unknown method '{}'", String::from_utf8_lossy(&method)),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct Request {
    method: Method,
    target: Bytes,
    version: Bytes,
    headers: HeaderMap,
    body: Body,
}

macro_rules! status_code {
    ($(($name:ident, $code:literal, $repr:literal)),+) => {
        impl StatusCode {
            $(
                pub const $name: StatusCode = StatusCode(unsafe {
                    NonZeroU16::new_unchecked($code)
                });
            )+

            // TODO: see optimizations in hyper
            #[inline]
            pub fn as_str(&self) -> &str {
                match self.as_u16() {
                    $($code => $repr,)+
                    code => unimplemented!("string representation of {code:?}"),
                }
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct StatusCode(NonZeroU16);

status_code! {
    (OK, 200, "OK"),
    (CREATED, 201, "Created"),
    (BAD_REQUEST, 400, "Bad Request"),
    (NOT_FOUND, 404, "Not Found"),
    (INTERNAL_SERVER_ERROR, 500, "Internal Server Error")
}

impl StatusCode {
    #[inline]
    pub fn as_u16(&self) -> u16 {
        self.0.into()
    }
}

impl Default for StatusCode {
    #[inline]
    fn default() -> Self {
        Self::OK
    }
}

#[derive(Debug)]
pub struct Response {
    pub(crate) version: Bytes,
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Body,
}

impl Response {
    #[inline]
    pub fn from_request(request: &Request) -> ResponseBuilder {
        let mut headers = HashMap::with_capacity(4);

        let accept_encoding = request.headers.extract::<AcceptEncoding>();

        if let Some(encoding) = accept_encoding.and_then(|enc| enc.into()) {
            headers.insert(CONTENT_ENCODING, encoding);
        }

        ResponseBuilder {
            version: request.version.clone(),
            status: StatusCode::default(),
            headers,
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
        let content_length = match body.content_length() {
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
    pub fn empty(self) -> Response {
        self.plain(Body::empty())
    }

    #[inline]
    pub fn plain(mut self, body: impl Into<Body>) -> Response {
        self = self.header(CONTENT_TYPE, TEXT_PLAIN);
        Self::build_response(self.version, self.status, self.headers, body.into())
    }

    pub async fn file(mut self, path: PathBuf) -> Response {
        let file = match fs::OpenOptions::new().read(true).open(path).await {
            Ok(file) => file,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied) => {
                return self.status(StatusCode::NOT_FOUND).empty()
            }
            Err(_) => return self.status(StatusCode::INTERNAL_SERVER_ERROR).empty(),
        };

        let body = match Body::file(file).await {
            Ok(body) => body,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied) => {
                return self.status(StatusCode::NOT_FOUND).empty()
            }
            Err(_) => return self.status(StatusCode::INTERNAL_SERVER_ERROR).empty(),
        };

        self = self.header(CONTENT_TYPE, OCTET_STREAM);

        Self::build_response(self.version, self.status, self.headers, body)
    }

    #[inline]
    pub fn build(self) -> Response {
        Self::build_response(self.version, self.status, self.headers, self.body.into())
    }
}

/// Handle a HTTP/1.1 client connection
pub async fn handle_connection(mut stream: TcpStream, cfg: &Config) -> Result<()> {
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
                    .plain(user_agent)
            },
        ),

        url if url.starts_with(b"/files") => {
            let file = url
                .strip_prefix(b"/files/")
                .filter(|f| !f.is_empty())
                .and_then(|f| std::str::from_utf8(f).map(Path::new).ok())
                .map(|f| cfg.files_dir().join(f));

            match (req.method, file) {
                (Method::Get, Some(file)) if file.is_file() => {
                    Response::from_request(&req)
                        .status(StatusCode::OK)
                        .file(file)
                        .await
                }

                (Method::Get, _) => Response::from_request(&req)
                    .status(StatusCode::NOT_FOUND)
                    .build(),

                (Method::Post, Some(file)) => upload_file(file, req).await,

                (Method::Post, None) => Response::from_request(&req)
                    .status(StatusCode::BAD_REQUEST)
                    .build(),
            }
        }

        url if url.starts_with(b"/echo") => {
            let msg = url.strip_prefix(b"/echo/").unwrap_or_default();

            Response::from_request(&req)
                .status(StatusCode::OK)
                .plain(msg)
        }

        _ => Response::from_request(&req)
            .status(StatusCode::NOT_FOUND)
            .build(),
    };

    println!("{resp:?}");

    writer.write_response(resp).await.context("write response")
}

async fn upload_file(file: PathBuf, req: Request) -> Response {
    let resp = Response::from_request(&req);

    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(file)
        .await;

    let mut file = match file {
        Ok(file) => FileWriter::new(file),
        Err(_) => return resp.status(StatusCode::INTERNAL_SERVER_ERROR).empty(),
    };

    let bytes_read = req.body.content_length();

    // TODO: stream body from the request based on Content-Type (i.e., don't materialize in memory)
    let Ok(bytes_written) = file.write(req.body).await else {
        return resp.status(StatusCode::INTERNAL_SERVER_ERROR).empty();
    };

    debug_assert_eq!(bytes_read, bytes_written, "corrupted file upload");

    resp.status(StatusCode::CREATED).build()
}
