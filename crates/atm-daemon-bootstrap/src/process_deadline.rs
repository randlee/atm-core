use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use atm_core::boundary::HookExitCause;
use atm_core::error::AtmError;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

pub(crate) struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
}

#[derive(Debug)]
pub(crate) struct SpawnedOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: ExitStatus,
    pub elapsed_ms: u64,
}

#[derive(Debug)]
pub(crate) enum SpawnFailure {
    TimedOut { after_ms: u64 },
    Failed(AtmError),
}

pub(crate) const STDOUT_CAP: u64 = 1 << 20;
pub(crate) const STDERR_CAP: u64 = 64 << 10;

struct GroupKillGuard {
    #[cfg(unix)]
    pgid: Option<rustix::process::Pid>,
    #[cfg(unix)]
    armed: bool,
}

impl GroupKillGuard {
    fn new(child: &Child) -> Self {
        #[cfg(not(unix))]
        let _ = child;
        Self {
            #[cfg(unix)]
            pgid: child
                .id()
                .and_then(|pid| i32::try_from(pid).ok())
                .and_then(rustix::process::Pid::from_raw),
            #[cfg(unix)]
            armed: true,
        }
    }

    fn kill(&self) {
        #[cfg(unix)]
        if self.armed
            && let Some(pgid) = self.pgid
        {
            let _ = rustix::process::kill_process_group(pgid, rustix::process::Signal::KILL);
        }
    }

    fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.armed = false;
        }
    }
}

impl Drop for GroupKillGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

fn failed(spec: &SpawnSpec, error: io::Error) -> SpawnFailure {
    SpawnFailure::Failed(AtmError::hook_exit(HookExitCause::Spawn {
        path: spec.program.clone(),
        cause: error.to_string(),
    }))
}

async fn read_capped(
    reader: impl AsyncRead + Unpin,
    cap: u64,
    name: &str,
) -> Result<Vec<u8>, SpawnFailure> {
    let mut bytes = Vec::new();
    reader
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| {
            SpawnFailure::Failed(AtmError::hook_exit(HookExitCause::Missing(format!(
                "cannot read {name}: {error}"
            ))))
        })?;
    if bytes.len() as u64 > cap {
        return Err(SpawnFailure::Failed(AtmError::hook_malformed(&format!(
            "{name} exceeds {cap} bytes"
        ))));
    }
    Ok(bytes)
}

async fn collect(
    child: &mut Child,
    stdin: &[u8],
    spec: &SpawnSpec,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), SpawnFailure> {
    let mut input = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let write = async {
        if let Err(error) = input.write_all(stdin).await
            && error.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(failed(spec, error));
        }
        drop(input);
        Ok(())
    };
    let wait = async { child.wait().await.map_err(|error| failed(spec, error)) };
    let (_, status, stdout, stderr) = tokio::try_join!(
        write,
        wait,
        read_capped(stdout, STDOUT_CAP, "stdout"),
        read_capped(stderr, STDERR_CAP, "stderr")
    )?;
    Ok((status, stdout, stderr))
}

pub(crate) async fn spawn_until(
    spec: SpawnSpec,
    stdin: &[u8],
    cancel: impl Future<Output = ()>,
) -> Result<SpawnedOutput, SpawnFailure> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(spec.env.iter().cloned())
        .current_dir(&spec.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let started = Instant::now();
    let mut child = command.spawn().map_err(|error| failed(&spec, error))?;
    let mut guard = GroupKillGuard::new(&child);
    let result = tokio::select! {
        biased;
        _ = cancel => Err(SpawnFailure::TimedOut { after_ms: 0 }),
        result = collect(&mut child, stdin, &spec) => result,
    };
    if result.is_err() {
        guard.kill();
        let _ = child.kill().await;
        child.wait().await.map_err(|error| failed(&spec, error))?;
    }
    guard.disarm();
    result.map(|(status, stdout, stderr)| SpawnedOutput {
        status,
        stdout,
        stderr,
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

pub(crate) async fn spawn_with_deadline(
    spec: SpawnSpec,
    stdin: &[u8],
    remaining: Duration,
) -> Result<SpawnedOutput, SpawnFailure> {
    spawn_until(spec, stdin, tokio::time::sleep(remaining))
        .await
        .map_err(|failure| match failure {
            SpawnFailure::TimedOut { .. } => SpawnFailure::TimedOut {
                after_ms: remaining.as_millis().min(u128::from(u64::MAX)) as u64,
            },
            other => other,
        })
}

#[cfg(test)]
pub(crate) fn environment_probe_output() -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "hook_runner::tests::environment_child_probe",
            "--nocapture",
        ])
        .env("ATM_BK3_LEAK_SENTINEL", "1")
        .output()
        .unwrap()
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::io::Read;

    use atm_core::atm_temp::{EnvSource, ProcessEnvSource};
    use atm_core::error::AtmErrorCode;
    use tempfile::TempDir;
    #[cfg(unix)]
    use tokio::sync::oneshot;

    use super::*;

    fn python() -> PathBuf {
        let path = ProcessEnvSource.var("PATH").expect("test PATH");
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(if cfg!(windows) {
                "python.exe"
            } else {
                "python3"
            });
            if candidate.is_file() {
                return candidate.canonicalize().unwrap();
            }
        }
        panic!("test interpreter missing")
    }

    fn spec(root: &TempDir, source: &str, args: Vec<OsString>) -> SpawnSpec {
        let module = root.path().join("child.py");
        fs::write(&module, source).unwrap();
        let mut arguments = vec![module.into_os_string()];
        arguments.extend(args);
        SpawnSpec {
            program: python(),
            args: arguments,
            env: ["PATH", "HOME", "SystemRoot", "USERPROFILE"]
                .into_iter()
                .filter_map(|key| {
                    ProcessEnvSource
                        .var(key)
                        .map(|value| (key.into(), value.into()))
                })
                .collect(),
            cwd: root.path().to_path_buf(),
        }
    }

    #[cfg(unix)]
    fn pipe(
        root: &TempDir,
    ) -> (
        PathBuf,
        oneshot::Receiver<()>,
        tokio::task::JoinHandle<Vec<u8>>,
    ) {
        let path = root.path().join("proof.fifo");
        assert!(
            std::process::Command::new(python())
                .args([
                    std::ffi::OsStr::new("-c"),
                    std::ffi::OsStr::new("import os,sys;os.mkfifo(sys.argv[1])"),
                    path.as_os_str()
                ])
                .status()
                .unwrap()
                .success()
        );
        let (ready, receiver) = oneshot::channel();
        let reader_path = path.clone();
        let reader = tokio::task::spawn_blocking(move || {
            let mut file = fs::File::open(reader_path).unwrap();
            let mut byte = [0];
            file.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [b'r']);
            ready.send(()).unwrap();
            let mut rest = Vec::new();
            file.read_to_end(&mut rest).unwrap();
            rest
        });
        (path, receiver, reader)
    }

    #[cfg(unix)]
    const GROUP_SOURCE: &str = "import os,sys,threading\nwriter=open(sys.argv[1],'wb',buffering=0)\nif os.fork()==0:\n threading.Event().wait()\nelse:\n writer.write(b'r')\n threading.Event().wait()\n";

    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_kills_child_and_grandchild() {
        let root = tempfile::tempdir().unwrap();
        let (path, ready, reader) = pipe(&root);
        let spawn = spec(&root, GROUP_SOURCE, vec![path.into_os_string()]);
        let (cancel, cancelled) = oneshot::channel();
        let handle = tokio::spawn(async move {
            spawn_until(spawn, b"{}", async {
                cancelled.await.unwrap();
            })
            .await
        });
        tokio::time::timeout(Duration::from_secs(10), ready)
            .await
            .expect("FIFO reader must observe the child readiness byte")
            .unwrap();
        cancel.send(()).unwrap();
        assert!(matches!(
            handle.await.unwrap(),
            Err(SpawnFailure::TimedOut { .. })
        ));
        assert!(
            tokio::time::timeout(Duration::from_secs(10), reader)
                .await
                .expect("FIFO reader must finish after child cancellation")
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn abort_kills_the_process_group() {
        let root = tempfile::tempdir().unwrap();
        let (path, ready, reader) = pipe(&root);
        let spawn = spec(&root, GROUP_SOURCE, vec![path.into_os_string()]);
        let handle =
            tokio::spawn(async move { spawn_until(spawn, b"{}", std::future::pending()).await });
        tokio::time::timeout(Duration::from_secs(10), ready)
            .await
            .expect("FIFO reader must observe the child readiness byte")
            .unwrap();
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
        assert!(
            tokio::time::timeout(Duration::from_secs(10), reader)
                .await
                .expect("FIFO reader must finish after process-group abort")
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_overflow_kills_both_fifo_writers_and_reaps_child() {
        let root = tempfile::tempdir().unwrap();
        let (path, ready, reader) = pipe(&root);
        let source = "import os,sys,threading\nwriter=open(sys.argv[1],'wb',buffering=0)\nif os.fork()==0:\n threading.Event().wait()\nelse:\n writer.write(b'r')\n sys.stdout.write('x'*((1<<20)+1))\n sys.stdout.flush()\n threading.Event().wait()\n";
        let spawn = spec(&root, source, vec![path.into_os_string()]);
        let handle =
            tokio::spawn(async move { spawn_until(spawn, b"{}", std::future::pending()).await });
        tokio::time::timeout(Duration::from_secs(10), ready)
            .await
            .expect("FIFO reader must observe the child readiness byte")
            .unwrap();
        let Err(SpawnFailure::Failed(error)) = handle.await.unwrap() else {
            panic!("expected cap error")
        };
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), reader)
                .await
                .expect("FIFO reader must finish after output overflow")
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn stderr_over_cap_is_bounded_and_malformed() {
        let root = tempfile::tempdir().unwrap();
        let spawn = spec(
            &root,
            "import sys,threading\nsys.stderr.write('x'*((64<<10)+1))\nsys.stderr.flush()\nthreading.Event().wait()\n",
            Vec::new(),
        );
        let Err(SpawnFailure::Failed(error)) =
            spawn_until(spawn, b"{}", std::future::pending()).await
        else {
            panic!("expected cap error")
        };
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(error.message().contains("stderr exceeds"));
    }

    #[tokio::test]
    async fn missing_program_is_a_typed_spawn_failure() {
        let root = tempfile::tempdir().unwrap();
        let mut spawn = spec(&root, "", Vec::new());
        spawn.program = root.path().join("absent-python");
        let Err(SpawnFailure::Failed(error)) =
            spawn_until(spawn, b"{}", std::future::pending()).await
        else {
            panic!("expected spawn error")
        };
        assert_eq!(error.code(), AtmErrorCode::HookExit);
        assert!(error.message().starts_with("HOOK.EXIT: cannot run "));
    }
}
