use std::io::Read;

use memchr::memmem::Finder;

use super::{HttpRequest, MAX_HTTP_HEADER_BYTES, MAX_HTTP_REQUEST_BODY_BYTES};
use crate::error::AtmError;

const HEADER_DELIMITER: &[u8] = b"\r\n\r\n";
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// Bounded request-frame reader for local HTTP transports.
///
/// A stream read can include more than one request. The reader retains the
/// exact bytes after the current request for the next call.
#[derive(Debug)]
pub struct HttpFrameReader {
    unread: Vec<u8>,
    delimiter: DelimiterFinder,
}

#[derive(Debug)]
enum DelimiterFinder {
    Optimized(Finder<'static>),
    #[cfg(test)]
    Scalar,
}

impl DelimiterFinder {
    fn find(&self, haystack: &[u8]) -> Option<usize> {
        match self {
            Self::Optimized(finder) => finder.find(haystack),
            #[cfg(test)]
            Self::Scalar => haystack
                .windows(HEADER_DELIMITER.len())
                .position(|window| window == HEADER_DELIMITER),
        }
    }
}

impl Default for HttpFrameReader {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpFrameReader {
    #[must_use]
    pub fn new() -> Self {
        Self {
            unread: Vec::with_capacity(READ_CHUNK_BYTES),
            delimiter: DelimiterFinder::Optimized(Finder::new(HEADER_DELIMITER)),
        }
    }

    #[cfg(test)]
    pub(super) fn scalar_for_test() -> Self {
        Self {
            unread: Vec::with_capacity(READ_CHUNK_BYTES),
            delimiter: DelimiterFinder::Scalar,
        }
    }

    /// Reads one complete local HTTP request frame.
    ///
    /// EOF before a frame is not an error. EOF inside a header or declared body
    /// remains a typed daemon-unavailable error, matching the legacy parser.
    pub fn read_request(
        &mut self,
        reader: &mut impl Read,
    ) -> Result<Option<HttpRequest>, AtmError> {
        let header_end = loop {
            if let Some(index) = self.delimiter.find(&self.unread) {
                break index + HEADER_DELIMITER.len();
            }
            if self.unread.len() > MAX_HTTP_HEADER_BYTES {
                return Err(AtmError::validation(
                    "daemon HTTP headers exceed 16384 bytes",
                ));
            }
            if !self.read_more(reader)? {
                return if self.unread.is_empty() {
                    Ok(None)
                } else {
                    Err(AtmError::daemon_unavailable(
                        "daemon HTTP headers ended unexpectedly",
                    ))
                };
            }
        };
        if header_end > MAX_HTTP_HEADER_BYTES {
            return Err(AtmError::validation(
                "daemon HTTP headers exceed 16384 bytes",
            ));
        }

        let (method, path, headers) = parse_headers(&self.unread[..header_end])?;
        let body_length = content_length(&headers)?;
        if body_length > MAX_HTTP_REQUEST_BODY_BYTES {
            return Err(AtmError::validation(
                "daemon HTTP body exceeds 1048576 bytes",
            ));
        }
        let frame_end = header_end + body_length;
        while self.unread.len() < frame_end {
            if !self.read_more(reader)? {
                return Err(AtmError::daemon_unavailable(
                    "failed to read daemon HTTP body: unexpected end of file",
                ));
            }
        }

        let body = self.unread[header_end..frame_end].to_vec();
        self.unread.drain(..frame_end);
        Ok(Some(HttpRequest {
            method,
            path,
            headers,
            body,
        }))
    }

    fn read_more(&mut self, reader: &mut impl Read) -> Result<bool, AtmError> {
        let mut chunk = [0_u8; READ_CHUNK_BYTES];
        match reader.read(&mut chunk) {
            Ok(0) => Ok(false),
            Ok(count) => {
                self.unread.extend_from_slice(&chunk[..count]);
                Ok(true)
            }
            Err(source) => Err(AtmError::daemon_unavailable(format!(
                "failed to read daemon HTTP headers: {source}",
            ))),
        }
    }
}

fn parse_headers(bytes: &[u8]) -> Result<(String, String, Vec<String>), AtmError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_source| AtmError::validation("daemon HTTP headers are not UTF-8"))?;
    let mut lines = text.split("\r\n");
    let start_line = lines.next().unwrap_or_default();
    let mut parts = start_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| AtmError::validation("malformed daemon HTTP request method"))?;
    let path = parts
        .next()
        .ok_or_else(|| AtmError::validation("malformed daemon HTTP request path"))?;
    if parts.next().is_none() {
        return Err(AtmError::validation(
            "malformed daemon HTTP request version",
        ));
    }
    Ok((
        method.to_string(),
        path.to_string(),
        lines
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    ))
}

fn content_length(headers: &[String]) -> Result<usize, AtmError> {
    headers
        .iter()
        .find_map(|header| {
            header
                .split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        })
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(|_source| AtmError::validation("daemon HTTP Content-Length is invalid"))
        .map(|length| length.unwrap_or(0))
}
