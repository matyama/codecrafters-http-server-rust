use std::collections::HashSet;
use std::process::Stdio;

use anyhow::{bail, Context, Error, Result};

use bytes::Bytes;
use itertools::Itertools as _;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::process::Command;

use crate::body::Body;

pub trait SystemEncoder {
    fn program(&self) -> Option<&str>;

    fn command(&self) -> Option<Command>;

    async fn compress(&self, body: Body) -> Result<Body> {
        let mut cmd = self.command().context("program is not configured")?;

        let cmd = match body {
            Body::Bytes(bytes) => {
                cmd.arg("-")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .kill_on_drop(true);

                let mut cmd =
                    tokio::task::spawn_blocking(move || cmd.spawn().context("spawn program"))
                        .await??;

                let input = cmd.stdin.take().context("setup program input")?;
                let mut input = BufWriter::new(input);

                input
                    .write_all(&bytes)
                    .await
                    .context("write program input")?;

                input.flush().await.context("flush program input")?;

                cmd
            }

            Body::File(file) => {
                cmd.arg(file.as_path())
                    .stdout(Stdio::piped())
                    .kill_on_drop(true);

                tokio::task::spawn_blocking(move || cmd.spawn().context("spawn program")).await??
            }
        };

        // XXX: for files it might be better to let the program write the output into a temp file
        //  - pros: don't have to load the whole (compressed) file contents into memory for output
        //  - cons: takes more storage space, have to deal with temp file cleanup and/or caching
        let output = cmd
            .wait_with_output()
            .await
            .context("wait for program output")?;

        if !output.status.success() {
            eprintln!("program exited with code {}", output.status);
        }

        Ok(Body::bytes(output.stdout))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoding {
    Gzip,
    Compress,
    Deflate,
    Br,
    Zstd,
}

macro_rules! encoding {
    ($($var:ident($name:ident, $enc:literal, $prg:expr)),+) => {
        impl Encoding {
            $(pub const $name: Bytes = Bytes::from_static($enc);)+

            #[inline]
            pub fn iter() -> impl Iterator<Item = Encoding> {
                [$(Self::$var),+].iter().cloned()
            }
        }

        impl TryFrom<&[u8]> for Encoding {
            type Error = Error;

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

        impl std::fmt::Display for Encoding {
            #[inline]
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        Self::$var => {
                            // SAFETY: ensured at the macro call site
                            debug_assert!(std::str::from_utf8($enc).is_ok());
                            write!(f, "{}", unsafe { std::str::from_utf8_unchecked($enc) })
                        }
                    )+
                }
            }
        }
    };
}

impl TryFrom<Bytes> for Encoding {
    type Error = Error;

    #[inline]
    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
        Self::try_from(bytes.as_ref())
    }
}

impl SystemEncoder for Encoding {
    #[inline]
    fn program(&self) -> Option<&str> {
        match self {
            Self::Gzip => Some("gzip"),
            Self::Compress => None,
            Self::Deflate => None,
            Self::Br => Some("brotli"),
            Self::Zstd => Some("zstd"),
        }
    }

    fn command(&self) -> Option<Command> {
        match self {
            Self::Gzip => self.program().map(Command::new).map(|mut gzip| {
                gzip.arg("-q").arg("-c");
                gzip
            }),
            Self::Compress => None,
            Self::Deflate => None,
            Self::Br => self.program().map(Command::new).map(|mut br| {
                br.arg("-c");
                br
            }),
            Self::Zstd => self.program().map(Command::new).map(|mut zstd| {
                zstd.arg("-q").arg("-c");
                zstd
            }),
        }
    }
}

// TODO: support * and quality values (e.g., `*;q=0.1`)
// https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept-Encoding
encoding! {
    Gzip(GZIP, b"gzip", Some("gzip")),
    Compress(COMPRESS, b"compress", None),
    Deflate(DEFLATE, b"deflate", None),
    Br(BR, b"br", Some("brotli")),
    Zstd(ZSTD, b"zstd", Some("zstd"))
}

pub(crate) fn get_supported() -> Result<HashSet<Encoding>> {
    use std::process::Command;

    let encs = Encoding::iter()
        .filter(|enc| enc.program().is_some())
        .collect_vec();

    let output = Command::new("whereis")
        .arg("-b") // locate just binaries
        .args(encs.iter().filter_map(SystemEncoder::program))
        .output()
        .context("query available programs")?;

    let output = String::from_utf8(output.stdout).context("invalid query output")?;

    let supported = encs
        .into_iter()
        .zip(output.split('\n'))
        .filter_map(|(enc, out)| {
            let (_, path) = out.split_once(':')?;
            if path.trim().is_empty() || path.contains("include") {
                None
            } else {
                Some(enc)
            }
        })
        .collect();

    Ok(supported)
}
