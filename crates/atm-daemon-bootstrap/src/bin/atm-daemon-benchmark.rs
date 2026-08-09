//! Feature-gated isolated benchmark entrypoint.
//!
//! The shipped `atm-daemon` binary cannot select a disabled received hook.
//! Capacity tooling must build this target with `benchmark-harness` and pass
//! an explicit subcommand on its command line.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::send::{SendMessageSource, WriteRequest, prepare_write_with_async_runtime};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_daemon_bootstrap::BenchmarkHookMode;
use atm_storage::{Message, MessageEnvelope, MessageKey};
use tokio::task::JoinSet;

const DIRECT_STORAGE_TEAM: &str = "capacity-direct-team";
const DIRECT_STORAGE_RECIPIENT: &str = "capacity-direct-recipient";
const DIRECT_STORAGE_SENDER: &str = "capacity-direct-agent";
const CORE_WRITE_TEAM: &str = "capacity-team";
const CORE_WRITE_RECIPIENT: &str = "capacity-recipient";
const CORE_WRITE_SENDER: &str = "capacity-agent";

#[derive(Debug)]
enum BenchmarkInvocation {
    Daemon(BenchmarkHookMode),
    DirectStorageAdmission {
        messages: NonZeroUsize,
        workers: NonZeroUsize,
    },
    DirectCoreWrite {
        messages: NonZeroUsize,
        workers: NonZeroUsize,
    },
}

#[tokio::main]
async fn main() {
    let result = match benchmark_invocation_from_args() {
        Ok(BenchmarkInvocation::Daemon(mode)) => {
            atm_daemon_bootstrap::run_benchmark_daemon(mode).await
        }
        Ok(BenchmarkInvocation::DirectStorageAdmission { messages, workers }) => {
            run_direct_storage_admission(messages, workers).await
        }
        Ok(BenchmarkInvocation::DirectCoreWrite { messages, workers }) => {
            run_direct_core_write(messages, workers).await
        }
        Err(error) => Err(error),
    };
    let exit_code = match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            replacement_exit_code(&error)
        }
    };
    std::process::exit(exit_code);
}

fn benchmark_invocation_from_args() -> Result<BenchmarkInvocation, AtmError> {
    parse_benchmark_invocation(std::env::args().skip(1))
}

fn parse_benchmark_invocation(
    args: impl IntoIterator<Item = String>,
) -> Result<BenchmarkInvocation, AtmError> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("--hook-mode") => {
            let mode = args.next().ok_or_else(|| {
                AtmError::config("usage: atm-daemon-benchmark --hook-mode <active|disabled>")
            })?;
            if args.next().is_some() {
                return Err(AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled>",
                ));
            }
            BenchmarkHookMode::parse(&mode).map(BenchmarkInvocation::Daemon)
        }
        Some("--direct-storage-admission") => {
            let (messages, workers) = parse_concurrent_arguments(
                args,
                "direct-storage",
                "usage: atm-daemon-benchmark --direct-storage-admission <count> --workers <count>",
            )?;
            Ok(BenchmarkInvocation::DirectStorageAdmission { messages, workers })
        }
        Some("--direct-core-write") => {
            let (messages, workers) = parse_concurrent_arguments(
                args,
                "direct-core-write",
                "usage: atm-daemon-benchmark --direct-core-write <count> --workers <count>",
            )?;
            Ok(BenchmarkInvocation::DirectCoreWrite { messages, workers })
        }
        _ => Err(AtmError::config(
            "usage: atm-daemon-benchmark --hook-mode <active|disabled> | --direct-storage-admission <count> --workers <count> | --direct-core-write <count> --workers <count>",
        )),
    }
}

fn parse_concurrent_arguments(
    mut args: impl Iterator<Item = String>,
    mode: &str,
    usage: &str,
) -> Result<(NonZeroUsize, NonZeroUsize), AtmError> {
    let messages = parse_nonzero_argument(args.next(), &format!("{mode} message count"))?;
    if args.next().as_deref() != Some("--workers") {
        return Err(AtmError::config(usage));
    }
    let workers = parse_nonzero_argument(args.next(), &format!("{mode} worker count"))?;
    if args.next().is_some() {
        return Err(AtmError::config(usage));
    }
    Ok((messages, workers))
}

fn parse_nonzero_argument(value: Option<String>, name: &str) -> Result<NonZeroUsize, AtmError> {
    value
        .ok_or_else(|| AtmError::config(format!("{name} is required")))?
        .parse::<usize>()
        .map_err(|_| AtmError::config(format!("{name} must be a positive integer")))
        .and_then(|value| {
            NonZeroUsize::new(value)
                .ok_or_else(|| AtmError::config(format!("{name} must be a positive integer")))
        })
}

/// Measures only the replacement daemon's async durable-admission seam.
///
/// This command is feature-gated with the daemon benchmark harness and is
/// invoked only after the Python harness has isolated the OS-user store. It
/// deliberately bypasses HTTP, JSON, capability validation, and core send
/// preparation so the resulting rate is the queue-plus-single-SQLite-writer
/// ceiling for the same `AsyncMessageStore` used by the Tokio daemon.
async fn run_direct_storage_admission(
    messages: NonZeroUsize,
    workers: NonZeroUsize,
) -> Result<(), AtmError> {
    let runtime = atm_daemon_bootstrap::assemble_default_runtime()?
        .for_daemon()
        .service_runtime;
    let next = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut tasks = JoinSet::new();

    for _ in 0..workers.get() {
        let runtime = runtime.clone();
        let next = Arc::clone(&next);
        tasks.spawn(async move {
            let mut accepted = 0_usize;
            loop {
                let sequence = next.fetch_add(1, Ordering::Relaxed);
                if sequence >= messages.get() {
                    return Ok::<usize, AtmError>(accepted);
                }
                let duplicate = runtime
                    .save_message_if_absent_async(direct_storage_message(sequence))
                    .await?;
                if duplicate.is_some() {
                    return Err(AtmError::daemon_unavailable(
                        "direct storage benchmark generated a duplicate message key",
                    ));
                }
                accepted += 1;
            }
        });
    }

    let mut accepted = 0_usize;
    while let Some(result) = tasks.join_next().await {
        accepted += result.map_err(|error| {
            AtmError::daemon_unavailable(format!("direct storage benchmark task failed: {error}"))
        })??;
    }
    let elapsed_seconds = started.elapsed().as_secs_f64();
    if accepted != messages.get() {
        return Err(AtmError::daemon_unavailable(format!(
            "direct storage benchmark accepted {accepted} of {} messages",
            messages
        )));
    }
    println!(
        "{}",
        serde_json::json!({
            "kind": "async_storage_admission",
            "requested_count": messages.get(),
            "accepted_count": accepted,
            "worker_count": workers.get(),
            "elapsed_seconds": elapsed_seconds,
            "admissions_per_second": accepted as f64 / elapsed_seconds,
        })
    );
    Ok(())
}

fn direct_storage_message(sequence: usize) -> Message {
    let team = TeamName::from_validated(DIRECT_STORAGE_TEAM);
    Message {
        team: team.clone(),
        agent: AgentName::from_validated(DIRECT_STORAGE_RECIPIENT),
        message_key: MessageKey::new(format!("atm:capacity-direct-{sequence}"))
            .expect("nonempty key"),
        envelope: MessageEnvelope {
            from: AgentName::from_validated(DIRECT_STORAGE_SENDER),
            source_chat_id: None,
            text: format!("capacity-direct-{sequence}"),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team),
            destination_chat_id: None,
            summary: None,
            message_id: None,
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        },
    }
}

/// Measures the shared canonical write preparation through the same Tokio
/// admission lane, excluding only HTTP parsing, authentication, and response
/// encoding. The capacity harness creates this mode's roster before invoking
/// it, so every write follows the normal warmed-roster local-send path.
async fn run_direct_core_write(
    messages: NonZeroUsize,
    workers: NonZeroUsize,
) -> Result<(), AtmError> {
    let runtime = atm_daemon_bootstrap::assemble_default_runtime()?
        .for_daemon()
        .service_runtime;
    let home = atm_core::home::atm_home()?;
    let next = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut tasks = JoinSet::new();

    for _ in 0..workers.get() {
        let runtime = runtime.clone();
        let home = home.clone();
        let next = Arc::clone(&next);
        tasks.spawn(async move {
            let mut accepted = 0_usize;
            loop {
                let sequence = next.fetch_add(1, Ordering::Relaxed);
                if sequence >= messages.get() {
                    return Ok::<usize, AtmError>(accepted);
                }
                prepare_write_with_async_runtime(
                    direct_core_write_request(&home, sequence)?,
                    &NullObservability,
                    &runtime,
                )
                .await?;
                accepted += 1;
            }
        });
    }

    let mut accepted = 0_usize;
    while let Some(result) = tasks.join_next().await {
        accepted += result.map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "direct core-write benchmark task failed: {error}"
            ))
        })??;
    }
    let elapsed_seconds = started.elapsed().as_secs_f64();
    if accepted != messages.get() {
        return Err(AtmError::daemon_unavailable(format!(
            "direct core-write benchmark accepted {accepted} of {} messages",
            messages.get()
        )));
    }
    println!(
        "{}",
        serde_json::json!({
            "kind": "canonical_core_write",
            "requested_count": messages.get(),
            "accepted_count": accepted,
            "worker_count": workers.get(),
            "elapsed_seconds": elapsed_seconds,
            "admissions_per_second": accepted as f64 / elapsed_seconds,
        })
    );
    Ok(())
}

fn direct_core_write_request(
    home: &std::path::Path,
    sequence: usize,
) -> Result<WriteRequest, AtmError> {
    WriteRequest::new(
        home.to_path_buf(),
        home.to_path_buf(),
        AgentName::from_validated(CORE_WRITE_SENDER),
        &format!("{CORE_WRITE_RECIPIENT}@{CORE_WRITE_TEAM}"),
        TeamName::from_validated(CORE_WRITE_TEAM),
        SendMessageSource::Inline(format!("capacity-core-{sequence}")),
        None,
        false,
        None,
        false,
    )
}

fn replacement_exit_code(error: &AtmError) -> i32 {
    if error.is_validation() || error.code() == atm_core::error::AtmErrorCode::DaemonUnavailable {
        64
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BenchmarkHookMode, BenchmarkInvocation, direct_core_write_request, direct_storage_message,
        parse_benchmark_invocation, parse_nonzero_argument,
    };

    #[test]
    fn direct_storage_messages_are_unique_and_target_the_capacity_recipient() {
        let first = direct_storage_message(1);
        let second = direct_storage_message(2);
        assert_ne!(first.message_key, second.message_key);
        assert_eq!(first.team.as_str(), "capacity-direct-team");
        assert_eq!(first.agent.as_str(), "capacity-direct-recipient");
        assert_eq!(
            first
                .envelope
                .source_team
                .as_ref()
                .map(|team| team.as_str()),
            Some("capacity-direct-team")
        );
    }

    #[test]
    fn direct_core_writes_use_the_canonical_capacity_address() {
        let home = std::path::Path::new("/tmp/atm-capacity");
        let request = direct_core_write_request(home, 7).expect("core request");
        assert_eq!(request.caller_identity.as_str(), "capacity-agent");
        assert_eq!(request.caller_team.as_str(), "capacity-team");
        assert_eq!(
            request.to.expect("destination").to_string(),
            "capacity-recipient@capacity-team"
        );
        assert_eq!(request.home_dir, home);
    }

    #[test]
    fn parser_rejects_zero_direct_storage_arguments() {
        let error = parse_nonzero_argument(Some("0".to_owned()), "count")
            .expect_err("zero must be rejected");
        assert!(error.message().contains("positive integer"));
    }

    #[test]
    fn parser_accepts_the_two_explicit_benchmark_modes() {
        let daemon = parse_benchmark_invocation(["--hook-mode".to_owned(), "disabled".to_owned()])
            .expect("hook-mode invocation parses");
        assert!(matches!(
            daemon,
            BenchmarkInvocation::Daemon(BenchmarkHookMode::Disabled)
        ));

        let direct = parse_benchmark_invocation([
            "--direct-storage-admission".to_owned(),
            "10000".to_owned(),
            "--workers".to_owned(),
            "64".to_owned(),
        ])
        .expect("direct-storage invocation parses");
        assert!(matches!(
            direct,
            BenchmarkInvocation::DirectStorageAdmission { messages, workers }
                if messages.get() == 10_000 && workers.get() == 64
        ));

        let core = parse_benchmark_invocation([
            "--direct-core-write".to_owned(),
            "10000".to_owned(),
            "--workers".to_owned(),
            "64".to_owned(),
        ])
        .expect("core invocation parses");
        assert!(matches!(
            core,
            BenchmarkInvocation::DirectCoreWrite { messages, workers }
                if messages.get() == 10_000 && workers.get() == 64
        ));
    }

    #[test]
    fn parser_rejects_ambiguous_direct_storage_invocations() {
        for arguments in [
            vec!["--direct-storage-admission", "100"],
            vec!["--direct-storage-admission", "100", "--workers", "0"],
            vec![
                "--direct-storage-admission",
                "100",
                "--workers",
                "2",
                "extra",
            ],
        ] {
            let error = parse_benchmark_invocation(arguments.into_iter().map(str::to_owned))
                .expect_err("ambiguous invocation must be rejected");
            assert!(
                error.message().contains("usage") || error.message().contains("positive integer")
            );
        }
    }
}
