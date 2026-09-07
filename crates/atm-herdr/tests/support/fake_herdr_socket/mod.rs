//! Test-only one-request Herdr socket fixture.

use std::io;

#[cfg(unix)]
use std::path::{Path, PathBuf};

#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
use tokio::net::UnixListener;

/// A deterministic one-request fake for the Unix transport lane.
#[cfg(unix)]
pub struct FakeHerdrSocket {
    path: PathBuf,
    listener: UnixListener,
}

#[cfg(unix)]
impl FakeHerdrSocket {
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let listener = UnixListener::bind(&path)?;
        Ok(Self { path, listener })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Accept exactly one request line and return the supplied response line.
    pub async fn serve_once(self, response: &[u8]) -> io::Result<Vec<u8>> {
        let (stream, _) = self.listener.accept().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut request = Vec::new();
        reader.read_until(b'\n', &mut request).await?;
        write_half.write_all(response).await?;
        Ok(request)
    }
}

#[cfg(unix)]
impl Drop for FakeHerdrSocket {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Windows uses Tokio's native named-pipe server in the platform lane.
#[cfg(windows)]
pub struct FakeHerdrSocket;

#[cfg(windows)]
impl FakeHerdrSocket {
    pub fn bind(_path: impl AsRef<std::ffi::OsStr>) -> io::Result<Self> {
        Ok(Self)
    }
}
