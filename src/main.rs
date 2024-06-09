use std::sync::Arc;

use anyhow::{Context, Result};
use itertools::Itertools;
use tokio::net::TcpListener;

use http_server_starter_rust::{handle_connection, Config};

#[tokio::main]
async fn main() -> Result<()> {
    println!("reading server configuration");
    let cfg = Config::from_args()
        .map(Arc::new)
        .context("parse program arguments")?;

    let encs = Config::encodings().iter().join(", ");
    println!("supported encodings: {encs}");

    let addr = cfg.listen_addr();

    println!("starting server at {addr}");
    let listener = TcpListener::bind(addr).await.context("bind TCP listener")?;

    println!("server is ready to accept connections");
    loop {
        tokio::select! {
            stream = listener.accept() => match stream {
                Ok((stream, addr)) => {
                    if let Err(e) = stream.set_nodelay(true) {
                        eprintln!("failed to enable TCP_NODELAY on connection: {e:?}");
                    }

                    let cfg = Arc::clone(&cfg);

                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, &cfg).await {
                            eprintln!("connection {addr} failed with {error}");
                        }
                    });
                }
                Err(error) => eprintln!("cannot get client: {error}"),
            }
        }
    }
}
