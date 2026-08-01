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
    ///
    /// Callers must configure a transport read deadline before calling this
    /// method: a generic [`Read`] cannot cancel a blocking read itself. Both
    /// production local transports enforce their three-second request deadline
    /// at their socket/worker boundary.
    pub fn read_request(
        &mut self,
        reader: &mut impl Read,
    ) -> Result<Option<HttpRequest>, AtmError> {
        // Only the delimiter overlap can become newly matchable after a read;
        // resuming there prevents repeatedly scanning an unbounded prefix.
        let mut search_from = 0;
        let header_end = loop {
            if let Some(index) = self.delimiter.find(&self.unread[search_from..]) {
                break search_from + index + HEADER_DELIMITER.len();
            }
            if self.unread.len() > MAX_HTTP_HEADER_BYTES {
                return Err(AtmError::validation_with_recovery(
                    format!("daemon HTTP headers exceed {MAX_HTTP_HEADER_BYTES} bytes"),
                    "send a smaller HTTP request header",
                ));
            }
            search_from = self
                .unread
                .len()
                .saturating_sub(HEADER_DELIMITER.len().saturating_sub(1));
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
            return Err(AtmError::validation_with_recovery(
                format!("daemon HTTP headers exceed {MAX_HTTP_HEADER_BYTES} bytes"),
                "send a smaller HTTP request header",
            ));
        }

        let (method, path, headers) = parse_headers(&self.unread[..header_end])?;
        let body_length = content_length(&headers)?;
        if body_length > MAX_HTTP_REQUEST_BODY_BYTES {
            return Err(AtmError::validation_with_recovery(
                format!("daemon HTTP body exceeds {MAX_HTTP_REQUEST_BODY_BYTES} bytes"),
                "send a smaller HTTP request body",
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
            Err(source) => Err(AtmError::daemon_unavailable_with_cause(
                "failed to read daemon HTTP headers",
                source,
            )),
        }
    }
}

fn parse_headers(bytes: &[u8]) -> Result<(String, String, Vec<String>), AtmError> {
    let text = std::str::from_utf8(bytes).map_err(|_source| {
        AtmError::validation_with_recovery(
            "daemon HTTP headers are not UTF-8",
            "send an HTTP/1.1 request encoded as UTF-8",
        )
    })?;
    let mut lines = text.split("\r\n");
    let start_line = lines.next().unwrap_or_default();
    let mut parts = start_line.split_whitespace();
    let method = parts.next().ok_or_else(|| {
        AtmError::validation_with_recovery(
            "malformed daemon HTTP request method",
            "send an HTTP/1.1 request line with method, path, and version",
        )
    })?;
    let path = parts.next().ok_or_else(|| {
        AtmError::validation_with_recovery(
            "malformed daemon HTTP request path",
            "send an HTTP/1.1 request line with method, path, and version",
        )
    })?;
    if parts.next().is_none() {
        return Err(AtmError::validation_with_recovery(
            "malformed daemon HTTP request version",
            "send an HTTP/1.1 request line with method, path, and version",
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
    let mut values = headers.iter().filter_map(|header| {
        header
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim())
    });
    let Some(value) = values.next() else {
        return Ok(0);
    };
    if values.next().is_some() {
        return Err(AtmError::validation_with_recovery(
            "daemon HTTP request contains duplicate Content-Length headers",
            "send exactly one Content-Length header",
        ));
    }
    value.parse().map_err(|_source| {
        AtmError::validation_with_recovery(
            "daemon HTTP Content-Length is invalid",
            "send one non-negative decimal Content-Length value",
        )
    })
}
