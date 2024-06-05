use anyhow::{Context, Result};
use tokio::net::TcpListener;

use http_server_starter_rust::handle_connection;

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
