use std::io::Read;

use memchr::memmem::Finder;

use super::{HttpRequest, MAX_HTTP_HEADER_BYTES, MAX_HTTP_REQUEST_BODY_BYTES};
use crate::error::AtmError;

const HEADER_DELIMITER: &[u8] = b"\r\n\r\n";
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// One complete bounded HTTP response frame.
///
/// A [`HttpFrameReader`] retains bytes belonging to a following frame, so a
/// caller can safely consume coalesced HTTP/1.1 responses without relying on
/// transport read boundaries.
#[derive(Debug)]
pub(crate) struct HttpResponseFrame {
    pub status_line: String,
    pub headers: Vec<String>,
    pub body: Vec<u8>,
}

/// Bounded HTTP/1.1 frame reader shared by local requests and remote responses.
///
/// A transport read can include more than one frame. The reader retains exact
/// trailing bytes for the next call and uses a chunked `memchr` delimiter scan,
/// never a byte-at-a-time system-read loop.
#[derive(Debug)]
pub struct HttpFrameReader {
    unread: Vec<u8>,
    delimiter: DelimiterFinder,
}

#[derive(Debug)]
enum DelimiterFinder {
    Optimized(Box<Finder<'static>>),
    #[cfg(test)]
    Scalar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Request,
    Response,
}

struct RawHttpFrame {
    start_line: String,
    headers: Vec<String>,
    body: Vec<u8>,
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
            delimiter: DelimiterFinder::Optimized(Box::new(Finder::new(HEADER_DELIMITER))),
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
        self.read_next(reader, FrameKind::Request)?
            .map(decode_request_frame)
            .transpose()
    }

    /// Reads one complete remote HTTP response frame.
    ///
    /// This has the same bounded, surplus-retaining framing semantics as
    /// [`Self::read_request`]. Remote adapters still own socket deadlines and
    /// mTLS verification; this type only owns HTTP framing.
    pub(crate) fn read_response(
        &mut self,
        reader: &mut impl Read,
    ) -> Result<Option<HttpResponseFrame>, AtmError> {
        self.read_next(reader, FrameKind::Response)?
            .map(|frame| {
                Ok(HttpResponseFrame {
                    status_line: frame.start_line,
                    headers: frame.headers,
                    body: frame.body,
                })
            })
            .transpose()
    }

    /// Consumes one complete request retained from an earlier stream read.
    pub fn read_buffered_request(&mut self) -> Result<Option<HttpRequest>, AtmError> {
        self.read_buffered_frame(FrameKind::Request)?
            .map(decode_request_frame)
            .transpose()
    }

    /// Consumes one complete response retained from an earlier stream read.
    #[cfg(test)]
    pub(super) fn read_buffered_response(&mut self) -> Result<Option<HttpResponseFrame>, AtmError> {
        self.read_buffered_frame(FrameKind::Response)?
            .map(|frame| {
                Ok(HttpResponseFrame {
                    status_line: frame.start_line,
                    headers: frame.headers,
                    body: frame.body,
                })
            })
            .transpose()
    }

    fn read_next(
        &mut self,
        reader: &mut impl Read,
        kind: FrameKind,
    ) -> Result<Option<RawHttpFrame>, AtmError> {
        loop {
            if let Some(frame) = self.read_buffered_frame(kind)? {
                return Ok(Some(frame));
            }
            if !self.read_more(reader)? {
                return if self.unread.is_empty() {
                    Ok(None)
                } else {
                    Err(AtmError::daemon_unavailable(unexpected_end_message(kind)))
                };
            }
        }
    }

    fn read_buffered_frame(&mut self, kind: FrameKind) -> Result<Option<RawHttpFrame>, AtmError> {
        let Some(header_index) = self.delimiter.find(&self.unread) else {
            if self.unread.len() > MAX_HTTP_HEADER_BYTES {
                return Err(header_limit_error(kind));
            }
            return Ok(None);
        };
        let header_end = header_index + HEADER_DELIMITER.len();
        if header_end > MAX_HTTP_HEADER_BYTES {
            return Err(header_limit_error(kind));
        }
        let (start_line, headers) = parse_header_lines(&self.unread[..header_end], kind)?;
        if kind == FrameKind::Request {
            request_parts(&start_line)?;
        }
        let body_length = content_length(&headers, kind)?;
        if body_length > MAX_HTTP_REQUEST_BODY_BYTES {
            return Err(body_limit_error(kind));
        }
        let frame_end = header_end + body_length;
        if self.unread.len() < frame_end {
            return Ok(None);
        }
        let body = self.unread[header_end..frame_end].to_vec();
        self.unread.drain(..frame_end);
        Ok(Some(RawHttpFrame {
            start_line,
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

fn decode_request_frame(frame: RawHttpFrame) -> Result<HttpRequest, AtmError> {
    let (method, path) = request_parts(&frame.start_line)?;
    Ok(HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers: frame.headers,
        body: frame.body,
    })
}

fn request_parts(start_line: &str) -> Result<(&str, &str), AtmError> {
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
    Ok((method, path))
}

fn parse_header_lines(bytes: &[u8], kind: FrameKind) -> Result<(String, Vec<String>), AtmError> {
    let text = std::str::from_utf8(bytes).map_err(|_source| match kind {
        FrameKind::Request => AtmError::validation_with_recovery(
            "daemon HTTP headers are not UTF-8",
            "send an HTTP/1.1 request encoded as UTF-8",
        ),
        FrameKind::Response => AtmError::validation("daemon HTTP headers are not UTF-8"),
    })?;
    let mut lines = text.split("\r\n");
    let start_line = lines.next().unwrap_or_default().to_string();
    Ok((
        start_line,
        lines
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    ))
}

fn content_length(headers: &[String], kind: FrameKind) -> Result<usize, AtmError> {
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
        return Err(match kind {
            FrameKind::Request => AtmError::validation_with_recovery(
                "daemon HTTP request contains duplicate Content-Length headers",
                "send exactly one Content-Length header",
            ),
            FrameKind::Response => AtmError::validation(
                "daemon HTTP response contains duplicate Content-Length headers",
            ),
        });
    }
    value.parse().map_err(|_source| match kind {
        FrameKind::Request => AtmError::validation_with_recovery(
            "daemon HTTP Content-Length is invalid",
            "send one non-negative decimal Content-Length value",
        ),
        FrameKind::Response => AtmError::validation("daemon HTTP Content-Length is invalid"),
    })
}

fn header_limit_error(kind: FrameKind) -> AtmError {
    match kind {
        FrameKind::Request => AtmError::validation_with_recovery(
            format!("daemon HTTP headers exceed {MAX_HTTP_HEADER_BYTES} bytes"),
            "send a smaller HTTP request header",
        ),
        FrameKind::Response => AtmError::validation(format!(
            "daemon HTTP headers exceed {MAX_HTTP_HEADER_BYTES} bytes"
        )),
    }
}

fn body_limit_error(kind: FrameKind) -> AtmError {
    match kind {
        FrameKind::Request => AtmError::validation_with_recovery(
            format!("daemon HTTP body exceeds {MAX_HTTP_REQUEST_BODY_BYTES} bytes"),
            "send a smaller HTTP request body",
        ),
        FrameKind::Response => AtmError::validation(format!(
            "daemon HTTP body exceeds {MAX_HTTP_REQUEST_BODY_BYTES} bytes"
        )),
    }
}

const fn unexpected_end_message(kind: FrameKind) -> &'static str {
    match kind {
        FrameKind::Request => "daemon HTTP request ended unexpectedly",
        FrameKind::Response => "daemon HTTP response ended unexpectedly",
    }
}
