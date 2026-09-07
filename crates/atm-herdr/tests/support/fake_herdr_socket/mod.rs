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

    /// Accept exactly one request line and return the supplied response line.
    pub async fn serve_once(self, response: Vec<u8>) -> io::Result<Vec<u8>> {
        let (stream, _) = self.listener.accept().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut request = Vec::new();
        reader.read_until(b'\n', &mut request).await?;
        write_half.write_all(&response).await?;
        Ok(request)
    }

    /// Accept one request and keep the connection open until the caller
    /// cancels the fixture. This is used to prove the client's read deadline
    /// and cancellation path without spawning a fixture-owned task.
    pub async fn serve_and_stall(self) -> io::Result<()> {
        let (stream, _) = self.listener.accept().await?;
        let (read_half, _write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut request = Vec::new();
        reader.read_until(b'\n', &mut request).await?;
        std::future::pending::<()>().await;
        Ok(())
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
pub struct FakeHerdrSocket {
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl FakeHerdrSocket {
    pub fn bind(path: impl AsRef<std::ffi::OsStr>) -> io::Result<Self> {
        let server = tokio::net::windows::named_pipe::ServerOptions::new()
            .max_instances(1)
            .create(path)?;
        Ok(Self { server })
    }

    pub async fn serve_once(mut self, response: Vec<u8>) -> io::Result<Vec<u8>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        self.server.connect().await?;
        let mut request = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            let read = self.server.read(&mut byte).await?;
            if read == 0 {
                break;
            }
            request.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        self.server.write_all(&response).await?;
        Ok(request)
    }

    pub async fn serve_and_stall(mut self) -> io::Result<()> {
        self.server.connect().await?;
        let mut request = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            let read = self.server.read(&mut byte).await?;
            if read == 0 {
                return Ok(());
            }
            request.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        std::future::pending::<()>().await;
        Ok(())
    }
}
