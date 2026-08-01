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
    Optimized(Box<Finder<'static>>),
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
        loop {
            if let Some(request) = self.read_buffered_request()? {
                return Ok(Some(request));
            }
            if !self.read_more(reader)? {
                return if self.unread.is_empty() {
                    Ok(None)
                } else {
                    Err(AtmError::daemon_unavailable(
                        "daemon HTTP request ended unexpectedly",
                    ))
                };
            }
        }
    }

    /// Consumes one complete request already retained from an earlier stream
    /// read without attempting another read from the transport.
    ///
    /// This lets a connection worker dispatch a bounded HTTP/1.1 pipeline
    /// while preserving the ordinary request/response behavior when a client
    /// has not sent another frame yet.
    pub fn read_buffered_request(&mut self) -> Result<Option<HttpRequest>, AtmError> {
        let Some(header_index) = self.delimiter.find(&self.unread) else {
            if self.unread.len() > MAX_HTTP_HEADER_BYTES {
                return Err(AtmError::validation_with_recovery(
                    format!("daemon HTTP headers exceed {MAX_HTTP_HEADER_BYTES} bytes"),
                    "send a smaller HTTP request header",
                ));
            }
            return Ok(None);
        };
        let header_end = header_index + HEADER_DELIMITER.len();
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
        if self.unread.len() < frame_end {
            return Ok(None);
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

#[cfg(test)]
mod ai51_campaign {
    use std::env;
    use std::io::{self, Read};

    use super::HttpFrameReader;

    struct PatternedReader {
        bytes: Vec<u8>,
        position: usize,
        chunks: Vec<usize>,
        index: usize,
    }
    impl PatternedReader {
        fn new(bytes: Vec<u8>, chunks: Vec<usize>) -> Self {
            Self {
                bytes,
                position: 0,
                chunks,
                index: 0,
            }
        }
    }
    impl Read for PatternedReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.position == self.bytes.len() {
                return Ok(0);
            }
            let count = (self.bytes.len() - self.position)
                .min(self.chunks[self.index % self.chunks.len()])
                .min(output.len());
            self.index += 1;
            output[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
            self.position += count;
            Ok(count)
        }
    }

    fn config() -> (u64, usize) {
        let seed = env::var("ATM_AI51_SEED")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(51_051);
        let cases = env::var("ATM_AI51_CASES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(128);
        assert!((1..=1_000).contains(&cases));
        (seed, cases)
    }
    fn next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        *state
    }
    fn chunks(state: &mut u64) -> Vec<usize> {
        (0..5).map(|_| ((next(state) % 31) + 1) as usize).collect()
    }
    fn post(body: &[u8]) -> Vec<u8> {
        let mut wire = format!(
            "POST /v1/atm/messages HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        wire.extend_from_slice(body);
        wire
    }
    fn body(state: &mut u64) -> Vec<u8> {
        (0..(next(state) % 97) as usize)
            .map(|_| b'a' + (next(state) % 26) as u8)
            .collect()
    }

    #[test]
    fn benign_fragment_and_coalesce() {
        let (mut state, cases) = config();
        for _ in 0..cases {
            let expected = body(&mut state);
            let mut reader = PatternedReader::new(post(&expected), chunks(&mut state));
            let request = HttpFrameReader::new()
                .read_request(&mut reader)
                .unwrap()
                .unwrap();
            assert_eq!(request.body, expected);
        }
    }
    #[test]
    fn candidate_replay() {
        let (mut state, cases) = config();
        for _ in 0..3 {
            for _ in 0..cases {
                let expected = body(&mut state);
                let first = post(&expected);
                let split = first.len() - 1;
                let mut wire = first;
                wire.extend_from_slice(b"GET /v1/atm/doctor HTTP/1.1\r\nContent-Length: 0\r\n\r\n");
                let mut reader = PatternedReader::new(wire, vec![split, 1, 2, 3]);
                let mut frames = HttpFrameReader::new();
                assert_eq!(
                    frames.read_request(&mut reader).unwrap().unwrap().body,
                    expected
                );
                assert_eq!(
                    frames.read_request(&mut reader).unwrap().unwrap().path,
                    "/v1/atm/doctor"
                );
            }
        }
    }
    #[test]
    fn known_boundaries() {
        for (mut wire, validation) in [
            (
                b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na" as &[u8],
                true,
            ),
            (b"POST / HTTP/1.1\r\nContent-Length: nope\r\n\r\n", true),
            (b"GET / HTTP/1.1\r\nX: \xff\r\n\r\n", true),
            (b"GET / HTTP/1.1\r\nContent-Length: 1\r\n\r\n", false),
        ] {
            let error = HttpFrameReader::new().read_request(&mut wire).unwrap_err();
            assert_eq!(error.is_validation(), validation);
            assert_eq!(error.is_daemon_unavailable(), !validation);
        }
    }
    #[test]
    fn optimized_scalar_parity() {
        let (mut state, cases) = config();
        for _ in 0..cases {
            let expected = body(&mut state);
            let bytes = post(&expected);
            let pattern = chunks(&mut state);
            let mut optimized = PatternedReader::new(bytes.clone(), pattern.clone());
            let mut scalar = PatternedReader::new(bytes, pattern);
            assert_eq!(
                HttpFrameReader::new().read_request(&mut optimized),
                HttpFrameReader::scalar_for_test().read_request(&mut scalar)
            );
        }
    }
}
