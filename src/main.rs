use anyhow::{Context, Result};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::net::{TcpListener, TcpStream};

const CRLF: &[u8] = b"\r\n";

async fn handle_connection(mut stream: TcpStream) -> Result<()> {
    let (_reader, writer) = stream.split();
    let mut writer = BufWriter::new(writer);

    // status
    let status = b"HTTP/1.1 200 OK";

    writer.write_all(status).await.context("status")?;
    writer.write_all(CRLF).await.context("status end")?;

    // headers
    writer.write_all(CRLF).await.context("headers end")?;

    // body

    writer.flush().await.context("flush response")
}

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:4221")
        .await
        .context("bind TCP listener")?;

    loop {
        tokio::select! {
            stream = listener.accept() => match stream {
                Ok((stream, addr)) => {
                    if let Err(error) = tokio::spawn(handle_connection(stream)).await {
                        eprintln!("connection {addr} failed with {error}");
                    }
                }
                Err(error) => eprintln!("cannot get client: {error}"),
            }
        }
    }
}
