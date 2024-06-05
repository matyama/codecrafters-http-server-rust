use std::io::{Cursor, Write as _};

use anyhow::{Context, Result};
use bytes::Bytes;
use tokio::io::{AsyncWriteExt, BufWriter};

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

        self.writer.write_all(CRLF).await.context("end")
    }

    pub async fn write_response(&mut self, response: Response) -> Result<()> {
        self.write_status_line(response.status, response.version)
            .await
            .context("status line")?;

        // TODO: headers
        self.writer.write_all(CRLF).await.context("headers end")?;

        if !response.body.is_empty() {
            self.writer
                .write_all(&response.body)
                .await
                .context("body")?;
        }

        self.writer.flush().await.context("flush")
    }
}
