//! Regression coverage for Unix CLI output whose downstream reader exits early.

#![cfg(unix)]

use std::process::{Command, Stdio};

#[test]
fn cli_does_not_panic_when_stdout_reader_closes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_atm"))
        .arg("help")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start atm help");

    drop(child.stdout.take());
    let output = child.wait_with_output().expect("wait for atm help");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "closed stdout must stop the child"
    );
    assert!(
        !stderr.contains("panicked at") && !stderr.contains("Broken pipe"),
        "closed stdout must not produce a Rust panic: {stderr}"
    );
}
