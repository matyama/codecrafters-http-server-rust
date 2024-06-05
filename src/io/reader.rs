use anyhow::{bail, Context, Result};
use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::header::{HeaderMap, CONTENT_LENGTH};
use crate::io::CRLF;
use crate::{Body, Request};

pub struct RequestReader<R> {
    reader: BufReader<R>,
    // here we'd ideally use some sort of buffer pooling
}

impl<R> RequestReader<R>
where
    R: AsyncReadExt + Send + Unpin,
{
    #[inline]
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    async fn read_segment(&mut self, buf: &mut BytesMut) -> Result<usize> {
        // TODO: ideally this would read directly into buf or use an inline buffer (i.e., no alloc)
        let mut aux = Vec::new();
        let mut len = 0;

        loop {
            let n = self.reader.read_until(b'\n', &mut aux).await?;
            len += n;
            if n > 0 && aux[..len].ends_with(CRLF) {
                break;
            }
        }

        buf.extend(aux);
        Ok(len)
    }

    async fn read_request_line(&mut self, buf: &mut BytesMut) -> Result<RequestLine> {
        let n = self.read_segment(buf).await?;

        // NOTE: strips trailing CRLF
        let mut req_line = buf.split_to(n - 2);
        let _ = buf.split_to(2);

        Ok(RequestLine {
            method: freeze_to_whitespace(&mut req_line),
            target: freeze_to_whitespace(&mut req_line),
            version: freeze_to_whitespace(&mut req_line),
        })
    }

    async fn read_header(&mut self, buf: &mut BytesMut) -> Result<Option<(Bytes, Bytes)>> {
        let n = self.read_segment(buf).await.context("header")?;

        let mut header = buf.split_to(n - 2); // strips trailing CRLF
        let _ = buf.split_to(2);

        if header.is_empty() {
            return Ok(None);
        }

        let Some(colon) = header.iter().position(|&b| b == b':') else {
            bail!("invalid header: {header:?}");
        };

        let mut value = header.split_off(colon).split_off(1); // strip colon
        if let Some(non_whitespace) = value.iter().position(|b| !b.is_ascii_whitespace()) {
            let _ = value.split_to(non_whitespace);
        }

        Ok(Some((header.freeze(), value.freeze())))
    }

    async fn read_headers(&mut self, buf: &mut BytesMut) -> Result<HeaderMap> {
        let mut headers = HeaderMap::builder();

        while let Some((name, value)) = self.read_header(buf).await? {
            headers.insert(name, value);
        }

        Ok(headers.build())
    }

    async fn read_body(&mut self, len: usize, buf: &mut BytesMut) -> Result<Body> {
        if len == 0 {
            return Ok(Body::empty());
        }

        buf.reserve(len);
        buf.resize(len, 0);

        self.reader.read_exact(&mut buf[..len]).await?;

        Ok(buf.split_to(len).into())
    }

    pub async fn read_request(&mut self) -> Result<Request> {
        let mut buf = BytesMut::with_capacity(1024);

        let RequestLine {
            method,
            target,
            version,
        } = self
            .read_request_line(&mut buf)
            .await
            .context("request line")?;

        let headers = self.read_headers(&mut buf).await.context("headers")?;

        // TODO: if we don't know body length after headers, then we should respond with 400/411
        // determine expected body length (https://stackoverflow.com/a/4826320)
        let content_length = headers.read(CONTENT_LENGTH).unwrap_or_default();

        let body = self
            .read_body(content_length, &mut buf)
            .await
            .context("body")?;

        Ok(Request {
            method,
            target,
            version,
            headers,
            body,
        })
    }
}

#[derive(Debug)]
struct RequestLine {
    method: Bytes,
    target: Bytes,
    version: Bytes,
}

fn freeze_to_whitespace(bytes: &mut BytesMut) -> Bytes {
    let at = bytes
        .iter()
        .position(|&b| b.is_ascii_whitespace())
        .unwrap_or(bytes.len());

    let result = bytes.split_to(at);

    if let Some(at) = bytes.iter().position(|&b| !b.is_ascii_whitespace()) {
        let _ = bytes.split_to(at);
    }

    result.freeze()
}
