//! One JSON object per line over a Unix socket, both ways.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

pub fn send_line(stream: &mut UnixStream, line: &str) -> io::Result<()> {
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

/// One line, or `UnexpectedEof` when the other side closed.
pub fn read_line(stream: &mut UnixStream) -> io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the other side closed the connection",
        ));
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_goes_through_and_a_closed_end_is_an_eof() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        send_line(&mut a, r#"{"x":1}"#).unwrap();
        assert_eq!(read_line(&mut b).unwrap(), "{\"x\":1}\n");
        drop(a);
        assert_eq!(
            read_line(&mut b).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
