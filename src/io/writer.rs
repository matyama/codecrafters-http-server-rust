use std::io::{Cursor, Write as _};

use anyhow::{Context, Result};
use bytes::Bytes;
use tokio::fs::File;
use tokio::io::{self, AsyncWriteExt, BufWriter};

use crate::body::Body;
use crate::header::HeaderMap;
use crate::io::CRLF;
use crate::{Response, StatusCode};

pub struct ResponseWriter<W> {
    writer: BufWriter<W>,
}

impl<W> ResponseWriter<W>
where
    W: AsyncWriteExt + Send + Unpin,
{
    #[inline]
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
        }
    }

    async fn write_status_line(&mut self, status: StatusCode, version: Bytes) -> Result<()> {
        self.writer.write_all(&version).await.context("version")?;

        self.writer.write_u8(b' ').await?;

        let mut buf = [0; 4];
        let mut w = Cursor::new(&mut buf[..]);
        let n = write!(w, "{} ", status.as_u16()).map(move |_| w.position())?;
        debug_assert_eq!(n, 4);

        self.writer.write_all(&buf).await.context("status code")?;

        self.writer
            .write_all(status.as_str().as_bytes())
            .await
            .context("status text")?;

        self.writer.write_all(CRLF).await.context("status end")
    }

    async fn write_header(&mut self, name: Bytes, value: Bytes) -> Result<()> {
        self.writer.write_all(&name).await.context("name")?;
        self.writer.write_all(b": ").await.context("separator")?;
        self.writer.write_all(&value).await.context("value")?;
        self.writer.write_all(CRLF).await.context("end")
    }

    async fn write_headers(&mut self, headers: HeaderMap) -> Result<()> {
        for (name, value) in headers.iter() {
            self.write_header(name, value).await?;
        }
        self.writer.write_all(CRLF).await.context("headers end")
    }

    pub async fn write_response(&mut self, response: Response) -> Result<()> {
        let response = response.compress().await;

        self.write_status_line(response.status, response.version)
            .await
            .context("status line")?;

        self.write_headers(response.headers)
            .await
            .context("headers")?;

        match response.body {
            body if body.is_empty() => {}

            Body::Bytes(body) => {
                self.writer.write_all(&body).await.context("body")?;
            }

            Body::File(body) => {
                let mut reader = body.into_reader();
                io::copy(&mut reader, &mut self.writer)
                    .await
                    .context("body")?;
            }
        }

        self.writer.flush().await.context("flush")
    }
}

#[repr(transparent)]
pub struct FileWriter(BufWriter<File>);

impl FileWriter {
    #[inline]
    pub fn new(file: File) -> Self {
        Self(BufWriter::new(file))
    }

    pub async fn write(&mut self, body: Body) -> io::Result<u64> {
        let n = match body {
            Body::Bytes(bytes) => {
                let mut reader = io::BufReader::new(Cursor::new(bytes));
                io::copy_buf(&mut reader, &mut self.0).await?
            }
            Body::File(file) => {
                let mut reader = file.into_reader();
                io::copy(&mut reader, &mut self.0).await?
            }
        };

        self.0.flush().await?;
        Ok(n)
    }
}
