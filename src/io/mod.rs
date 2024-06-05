pub(crate) mod reader;
pub(crate) mod writer;

pub(crate) use reader::RequestReader;
pub(crate) use writer::ResponseWriter;

pub(crate) const CRLF: &[u8] = b"\r\n";
