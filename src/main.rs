use std::io::{self, Write as _};
use std::ops::RangeInclusive;
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{TcpListener, TcpStream};

const SEP: u8 = b' ';
const LF: u8 = b'\n';
const CRLF: &[u8] = b"\r\n";

const ROOT: &[u8] = b"/";

const OK: Bytes = Bytes::from_static(b"OK");
const NOT_FOUND: Bytes = Bytes::from_static(b"Not Found");

const EMPTY: Bytes = Bytes::from_static(&[]);

const STATUS_RANGE: RangeInclusive<u16> = 100..=500;

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct Request {
    // TODO: method as an enum
    method: Bytes,
    target: Bytes,
    version: Bytes,
    headers: Arc<[(Bytes, Bytes)]>,
    body: Bytes,
}

async fn read_segment<R>(reader: &mut R, buf: &mut BytesMut) -> Result<usize>
where
    R: AsyncBufReadExt + Unpin,
{
    // TODO: ideally this would read directly into buf or use an inline buffer (i.e., no alloc)
    let mut aux = Vec::new();
    let mut len = 0;

    loop {
        let n = reader.read_until(LF, &mut aux).await?;
        len += n;
        if n > 0 && aux[..len].ends_with(CRLF) {
            break;
        }
    }

    buf.extend(aux);
    Ok(len)
}

async fn read_body<R>(len: usize, reader: &mut R, buf: &mut BytesMut) -> Result<Bytes>
where
    R: AsyncBufReadExt + Unpin,
{
    if len == 0 {
        return Ok(EMPTY);
    }

    buf.reserve(len);
    buf.resize(len, 0);

    reader.read_exact(&mut buf[..len]).await?;

    Ok(buf.split_to(len).freeze())
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

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    println!("handling new connection");

    let (reader, writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    let mut buf = BytesMut::with_capacity(512);

    // TODO: extract to RequestReader

    // read request line
    let n = read_segment(&mut reader, &mut buf)
        .await
        .context("read status line")?;

    let mut req_line = buf.split_to(n - 2); // strips trailing CRLF

    let method = freeze_to_whitespace(&mut req_line);
    let target = freeze_to_whitespace(&mut req_line);
    let version = freeze_to_whitespace(&mut req_line);

    // skip separating CRLF
    let _ = buf.split_to(2);

    // read headers

    // determine expected body length (https://stackoverflow.com/a/4826320)
    let mut content_length = None;

    let mut headers = Vec::new();
    loop {
        let n = read_segment(&mut reader, &mut buf)
            .await
            .context("read status line")?;

        let mut header = buf.split_to(n - 2); // strips trailing CRLF
        let _ = buf.split_to(2);

        if header.is_empty() {
            break;
        }

        let Some(colon) = header.iter().position(|&b| b == b':') else {
            bail!("invalid header: {header:?}");
        };

        let mut value = header.split_off(colon).split_off(1); // strip colon
        if let Some(non_whitespace) = value.iter().position(|b| !b.is_ascii_whitespace()) {
            let _ = value.split_to(non_whitespace);
        }

        if matches!(header.as_ref(), b"Content-Length") {
            let length = std::str::from_utf8(&value)
                .with_context(|| format!("invalid Content-Length value {value:?}"))?;

            let length = length
                .parse()
                .with_context(|| format!("invalid Content-Length value {length:?}"))?;

            // TODO: handle duplicate Content-Length header
            content_length = Some(length);
        }

        headers.push((header.freeze(), value.freeze()));
    }

    // TODO: realistically, if we don't know it after headers, then we should respond with 400/411
    let content_length = content_length.unwrap_or_default();

    // read body
    let body = read_body(content_length, &mut reader, &mut buf)
        .await
        .context("read body")?;

    let req = Request {
        method,
        target,
        version,
        headers: Arc::from(headers.into_boxed_slice()),
        body,
    };

    println!("{req:?}");

    // TODO: extract to ResponseWriter

    // status

    let (code, status) = match req.target.as_ref() {
        ROOT => (200, OK),
        _ => (404, NOT_FOUND),
    };

    ensure!(
        STATUS_RANGE.contains(&code),
        "status code {code} is out of range {STATUS_RANGE:?}"
    );

    writer
        .write_all(&req.version)
        .await
        .context("write version")?;

    writer.write_u8(SEP).await?;

    let mut buf = [0; 4];
    let mut w = io::Cursor::new(&mut buf[..]);
    let n = write!(w, "{code} ").map(move |_| w.position())?;
    debug_assert_eq!(n, 4);

    writer.write_all(&buf).await.context("write status code")?;

    writer.write_all(&status).await.context("write status")?;

    writer.write_all(CRLF).await.context("status end")?;

    // headers
    writer.write_all(CRLF).await.context("headers end")?;

    // body

    writer.flush().await.context("flush response")
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:4221";

    println!("starting server at {addr}");
    let listener = TcpListener::bind(addr).await.context("bind TCP listener")?;

    println!("server is ready to accept connections");
    loop {
        tokio::select! {
            stream = listener.accept() => match stream {
                Ok((stream, addr)) => {
                    stream.set_nodelay(true)?;
                    if let Err(error) = tokio::spawn(handle_connection(stream)).await {
                        eprintln!("connection {addr} failed with {error}");
                    }
                }
                Err(error) => eprintln!("cannot get client: {error}"),
            }
        }
    }
}
