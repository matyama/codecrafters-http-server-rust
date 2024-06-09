use std::collections::HashSet;
use std::env::Args;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{bail, Context as _, Result};

use crate::encoding::{self, Encoding};

fn listen_socket_addr(port: &impl std::fmt::Display) -> Result<SocketAddr> {
    format!("0.0.0.0:{port}")
        .parse()
        .with_context(|| format!("failed to parse listen socket address: '0.0.0.0:{port}'"))
}

#[derive(Debug)]
pub struct Config {
    pub(crate) addr: SocketAddr,
    pub(crate) dir: PathBuf,
}

impl Config {
    #[inline]
    pub fn from_args() -> Result<Self> {
        std::env::args().try_into()
    }

    #[inline]
    pub fn listen_addr(&self) -> SocketAddr {
        self.addr
    }

    #[inline]
    pub fn files_dir(&self) -> &Path {
        self.dir.as_path()
    }

    #[inline]
    pub fn encodings() -> &'static HashSet<Encoding> {
        // NOTE: Normally, this would not be necessary, but here we depend on external programs.
        static SUPPORTED: OnceLock<HashSet<Encoding>> = OnceLock::new();
        SUPPORTED.get_or_init(|| {
            encoding::get_supported().expect("query available compression programs")
        })
    }
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            addr: listen_socket_addr(&4221).expect("default listen address"),
            dir: PathBuf::from("/tmp"),
        }
    }
}

impl TryFrom<Args> for Config {
    type Error = anyhow::Error;

    fn try_from(args: Args) -> Result<Self> {
        let mut args = args.into_iter().skip(1);

        let mut cfg = Self::default();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--port" | "-p" => {
                    let Some(port) = args.next() else {
                        bail!("missing argument value for --port");
                    };

                    let Ok(addr) = listen_socket_addr(&port) else {
                        bail!("invalid argument value for --port: '{port}'");
                    };

                    cfg.addr = addr;
                }

                "--dir" | "--directory" => {
                    let Some(dir) = args.next().map(PathBuf::from) else {
                        bail!("missing argument value for --dir");
                    };

                    cfg.dir = dir;
                }

                _ => continue,
            }
        }

        Ok(cfg)
    }
}
