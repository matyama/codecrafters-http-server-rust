use anyhow::{Context as _, Result};
use tokio::net::{TcpListener, TcpStream};

async fn handle_connection(_stream: TcpStream) -> Result<()> {
    println!("handled new connection");
    Ok(())
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
