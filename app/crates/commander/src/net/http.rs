//! A minimal HTTP/1.1 GET over a TCP stream, enough for the addin's `Connection: close`
//! replies: status, headers, a body that is either chunked or runs to the end of the stream.

use anyhow::{anyhow, bail, Context};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// Header names lower-cased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// `GET path`, with `If-None-Match` when an ETag is known.
pub async fn get(
    host: &str,
    port: u16,
    path: &str,
    if_none_match: Option<&str>,
) -> anyhow::Result<Response> {
    let io = async {
        let mut stream = TcpStream::connect((host, port))
            .await
            .with_context(|| format!("connect {host}:{port}"))?;
        let mut request = format!(
            "GET {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: commander\r\n",
            encode_path(path)
        );
        if let Some(etag) = if_none_match {
            request.push_str(&format!("If-None-Match: {etag}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).await?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await?;
        parse_response(&raw)
    };
    tokio::time::timeout(TIMEOUT, io)
        .await
        .map_err(|_| anyhow!("GET {path}: timed out"))?
}

/// Percent-encodes everything but unreserved characters and `/`.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Splits a raw reply into status, headers and body.
pub fn parse_response(raw: &[u8]) -> anyhow::Result<Response> {
    let split = find(raw, b"\r\n\r\n").ok_or_else(|| anyhow!("no header terminator"))?;
    let head = std::str::from_utf8(&raw[..split]).context("headers are not UTF-8")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("bad status line {status_line:?}"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    let rest = &raw[split + 4..];
    let chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
    let body = if chunked {
        dechunk(rest)?
    } else {
        let len = headers
            .iter()
            .find(|(k, _)| k == "content-length")
            .and_then(|(_, v)| v.parse::<usize>().ok());
        match len {
            Some(n) if n <= rest.len() => rest[..n].to_vec(),
            Some(n) => bail!("body truncated: {} of {n} bytes", rest.len()),
            None => rest.to_vec(),
        }
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(mut rest: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = find(rest, b"\r\n").ok_or_else(|| anyhow!("chunk size line missing"))?;
        let size_text = std::str::from_utf8(&rest[..eol])?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).context("chunk size")?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size + 2 {
            bail!("chunk truncated");
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_reply() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nETag: \"123-456\"\r\nConnection: close\r\n\r\nhello";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.header("etag"), Some("\"123-456\""));
        assert_eq!(r.header("ETag"), Some("\"123-456\""));
        assert_eq!(r.body, b"hello");
    }

    #[test]
    fn content_length_and_304() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdef";
        assert_eq!(parse_response(raw).unwrap().body, b"abc");
        let raw = b"HTTP/1.1 304 Not Modified\r\nETag: x\r\n\r\n";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 304);
        assert!(r.body.is_empty());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc").is_err());
    }

    #[test]
    fn chunked_reply() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;ext=1\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).unwrap().body, b"abcde");
    }

    #[test]
    fn path_encoding() {
        assert_eq!(encode_path("/skin/3/TUI.json"), "/skin/3/TUI.json");
        assert_eq!(
            encode_path("/skin/3/Q-Links - 8by1.json"),
            "/skin/3/Q-Links%20-%208by1.json"
        );
        assert_eq!(encode_path("/a#b"), "/a%23b");
    }
}
