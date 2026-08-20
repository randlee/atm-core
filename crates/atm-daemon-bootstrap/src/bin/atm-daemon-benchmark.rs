//! Feature-gated isolated benchmark entrypoint.
//!
//! The shipped `atm-daemon` binary cannot select a disabled received hook.
//! Capacity tooling must build this target with `benchmark-harness` and pass
//! an explicit subcommand on its command line.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::{env, process};

use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::send::{SendMessageSource, WriteRequest, prepare_write_with_async_runtime};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_daemon_bootstrap::{
    BenchmarkDirectPeerListener, BenchmarkHookMode, BenchmarkPeerWireSecurity,
};
use atm_storage::{Message, MessageEnvelope, MessageKey};
use tokio::task::JoinSet;

const DIRECT_STORAGE_TEAM: &str = "capacity-direct-team";
const DIRECT_STORAGE_RECIPIENT: &str = "capacity-direct-recipient";
const DIRECT_STORAGE_SENDER: &str = "capacity-direct-agent";
const CORE_WRITE_TEAM: &str = "capacity-core-team";
const CORE_WRITE_RECIPIENT: &str = "capacity-core-recipient";
const CORE_WRITE_SENDER: &str = "capacity-core-agent";
const DIRECT_PROBE_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
enum BenchmarkInvocation {
    Daemon {
        hook_mode: BenchmarkHookMode,
        peer_wire_security: BenchmarkPeerWireSecurity,
        direct_peer_listener: BenchmarkDirectPeerListener,
    },
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
        Ok(BenchmarkInvocation::Daemon {
            hook_mode,
            peer_wire_security,
            direct_peer_listener,
        }) => {
            atm_daemon_bootstrap::run_benchmark_daemon(
                hook_mode,
                peer_wire_security,
                direct_peer_listener,
            )
            .await
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
                AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled>",
                )
            })?;
            if args.next().as_deref() != Some("--peer-wire-security") {
                return Err(AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled>",
                ));
            }
            let peer_wire_security = args.next().ok_or_else(|| {
                AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled>",
                )
            })?;
            if args.next().as_deref() != Some("--direct-peer-listener") {
                return Err(AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled>",
                ));
            }
            let direct_peer_listener = args.next().ok_or_else(|| {
                AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled>",
                )
            })?;
            if args.next().is_some() {
                return Err(AtmError::config(
                    "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled>",
                ));
            }
            Ok(BenchmarkInvocation::Daemon {
                hook_mode: BenchmarkHookMode::parse(&mode)?,
                peer_wire_security: BenchmarkPeerWireSecurity::parse(&peer_wire_security)?,
                direct_peer_listener: BenchmarkDirectPeerListener::parse(&direct_peer_listener)?,
            })
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
            "usage: atm-daemon-benchmark --hook-mode <active|disabled> --peer-wire-security <plaintext-test|mtls> --direct-peer-listener <disabled|enabled> | --direct-storage-admission <count> --workers <count> | --direct-core-write <count> --workers <count>",
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
    let run_id = env::var("ATM_CAPACITY_RUN_ID").unwrap_or_else(|_| process::id().to_string());
    let write_config = DirectStorageWriteConfig::new(run_id);
    let next = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut tasks = JoinSet::new();

    for _ in 0..workers.get() {
        let runtime = runtime.clone();
        let next = Arc::clone(&next);
        let write_config = write_config.clone();
        tasks.spawn(async move {
            let mut accepted = 0_usize;
            loop {
                let sequence = next.fetch_add(1, Ordering::Relaxed);
                if sequence >= messages.get() {
                    return Ok::<usize, DirectProbeWorkerFailure>(accepted);
                }
                let duplicate = tokio::time::timeout(
                    DIRECT_PROBE_WRITE_TIMEOUT,
                    runtime.save_message_if_absent_async(direct_storage_message(
                        &write_config,
                        sequence,
                    )),
                )
                .await
                .map_err(|_| DirectProbeWorkerFailure::timeout(accepted, "direct storage"))?
                .map_err(|error| DirectProbeWorkerFailure::new(accepted, error))?;
                if duplicate.is_some() {
                    return Err(DirectProbeWorkerFailure::new(
                        accepted,
                        AtmError::daemon_unavailable(
                            "direct storage benchmark generated a duplicate message key",
                        ),
                    ));
                }
                accepted += 1;
            }
        });
    }

    let accepted = collect_direct_probe_workers(tasks, "direct storage").await?;
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

/// Immutable direct-storage message inputs, built once before worker launch.
#[derive(Clone)]
struct DirectStorageWriteConfig {
    run_id: String,
    team: TeamName,
    recipient: AgentName,
    sender: AgentName,
}

impl DirectStorageWriteConfig {
    fn new(run_id: String) -> Self {
        Self {
            run_id,
            team: TeamName::from_validated(DIRECT_STORAGE_TEAM),
            recipient: AgentName::from_validated(DIRECT_STORAGE_RECIPIENT),
            sender: AgentName::from_validated(DIRECT_STORAGE_SENDER),
        }
    }
}

fn direct_storage_message(config: &DirectStorageWriteConfig, sequence: usize) -> Message {
    let team = config.team.clone();
    Message {
        team: team.clone(),
        agent: config.recipient.clone(),
        message_key: MessageKey::new(format!("atm:capacity-direct-{}-{sequence}", config.run_id))
            .expect("nonempty key"),
        envelope: MessageEnvelope {
            from: config.sender.clone(),
            source_chat_id: None,
            text: format!("capacity-direct-{}-{sequence}", config.run_id),
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
    let request_config = DirectCoreWriteConfig::from_environment(atm_core::home::atm_home()?);
    let next = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut tasks = JoinSet::new();

    for _ in 0..workers.get() {
        let runtime = runtime.clone();
        let request_config = request_config.clone();
        let next = Arc::clone(&next);
        tasks.spawn(async move {
            let mut accepted = 0_usize;
            loop {
                let sequence = next.fetch_add(1, Ordering::Relaxed);
                if sequence >= messages.get() {
                    return Ok::<usize, DirectProbeWorkerFailure>(accepted);
                }
                let request = direct_core_write_request(&request_config, sequence)
                    .map_err(|error| DirectProbeWorkerFailure::new(accepted, error))?;
                tokio::time::timeout(
                    DIRECT_PROBE_WRITE_TIMEOUT,
                    prepare_write_with_async_runtime(request, &NullObservability, &runtime),
                )
                .await
                .map_err(|_| DirectProbeWorkerFailure::timeout(accepted, "direct core-write"))?
                .map_err(|error| DirectProbeWorkerFailure::new(accepted, error))?;
                accepted += 1;
            }
        });
    }

    let accepted = collect_direct_probe_workers(tasks, "direct core-write").await?;
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

#[derive(Debug)]
struct DirectProbeWorkerFailure {
    accepted: usize,
    error: AtmError,
}

impl DirectProbeWorkerFailure {
    fn new(accepted: usize, error: AtmError) -> Self {
        Self { accepted, error }
    }

    fn timeout(accepted: usize, kind: &str) -> Self {
        Self::new(
            accepted,
            AtmError::daemon_unavailable(format!(
                "{kind} benchmark write exceeded the {}s per-write deadline",
                DIRECT_PROBE_WRITE_TIMEOUT.as_secs(),
            )),
        )
    }
}

/// Drain every worker after the first error so the emitted error contains the
/// exact number of writes each completed/failed worker durably admitted.
async fn collect_direct_probe_workers(
    mut tasks: JoinSet<Result<usize, DirectProbeWorkerFailure>>,
    kind: &str,
) -> Result<usize, AtmError> {
    let mut accepted = 0_usize;
    let mut first_error = None;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(count)) => accepted += count,
            Ok(Err(error)) => {
                accepted += error.accepted;
                if first_error.is_none() {
                    first_error = Some(error.error);
                }
            }
            Err(error) if first_error.is_none() => {
                first_error = Some(AtmError::daemon_unavailable(format!(
                    "{kind} benchmark task failed: {error}"
                )));
            }
            Err(_) => {}
        }
    }
    if let Some(error) = first_error {
        return Err(AtmError::daemon_unavailable(format!(
            "{kind} benchmark failed after durably admitting {accepted} messages: {error}"
        )));
    }
    Ok(accepted)
}

/// Environment-derived benchmark identities captured once before concurrent
/// workers start. The hot admission loop must not repeatedly read mutable
/// process state or re-allocate its target address per message.
#[derive(Clone)]
struct DirectCoreWriteConfig {
    home: PathBuf,
    sender: AgentName,
    recipient_address: String,
    team: TeamName,
}

impl DirectCoreWriteConfig {
    fn from_environment(home: PathBuf) -> Self {
        let team =
            env::var("ATM_CAPACITY_CORE_TEAM").unwrap_or_else(|_| CORE_WRITE_TEAM.to_owned());
        let sender =
            env::var("ATM_CAPACITY_CORE_AGENT").unwrap_or_else(|_| CORE_WRITE_SENDER.to_owned());
        let recipient = env::var("ATM_CAPACITY_CORE_RECIPIENT")
            .unwrap_or_else(|_| CORE_WRITE_RECIPIENT.to_owned());
        Self {
            home,
            sender: AgentName::from_validated(&sender),
            recipient_address: format!("{recipient}@{team}"),
            team: TeamName::from_validated(&team),
        }
    }
}

fn direct_core_write_request(
    config: &DirectCoreWriteConfig,
    sequence: usize,
) -> Result<WriteRequest, AtmError> {
    WriteRequest::new(
        config.home.clone(),
        config.home.clone(),
        config.sender.clone(),
        &config.recipient_address,
        config.team.clone(),
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
        BenchmarkDirectPeerListener, BenchmarkHookMode, BenchmarkInvocation,
        BenchmarkPeerWireSecurity, DirectCoreWriteConfig, DirectStorageWriteConfig,
        direct_core_write_request, direct_storage_message, parse_benchmark_invocation,
        parse_nonzero_argument,
    };

    #[test]
    fn direct_storage_messages_are_unique_and_target_the_capacity_recipient() {
        let config = DirectStorageWriteConfig::new("test-run".to_owned());
        let first = direct_storage_message(&config, 1);
        let second = direct_storage_message(&config, 2);
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
        let home = std::env::temp_dir().join("atm-capacity");
        let config = DirectCoreWriteConfig::from_environment(home.clone());
        let request = direct_core_write_request(&config, 7).expect("core request");
        assert_eq!(request.caller_identity.as_str(), "capacity-core-agent");
        assert_eq!(request.caller_team.as_str(), "capacity-core-team");
        assert_eq!(
            request.to.expect("destination").to_string(),
            "capacity-core-recipient@capacity-core-team"
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
        let daemon = parse_benchmark_invocation([
            "--hook-mode".to_owned(),
            "disabled".to_owned(),
            "--peer-wire-security".to_owned(),
            "mtls".to_owned(),
            "--direct-peer-listener".to_owned(),
            "enabled".to_owned(),
        ])
        .expect("hook-mode invocation parses");
        assert!(matches!(
            daemon,
            BenchmarkInvocation::Daemon {
                hook_mode: BenchmarkHookMode::Disabled,
                peer_wire_security: BenchmarkPeerWireSecurity::MutualTls,
                direct_peer_listener: BenchmarkDirectPeerListener::Enabled,
            }
        ));

        let plaintext = parse_benchmark_invocation([
            "--hook-mode".to_owned(),
            "disabled".to_owned(),
            "--peer-wire-security".to_owned(),
            "plaintext-test".to_owned(),
            "--direct-peer-listener".to_owned(),
            "enabled".to_owned(),
        ])
        .expect("explicit plaintext benchmark invocation parses");
        assert!(matches!(
            plaintext,
            BenchmarkInvocation::Daemon {
                hook_mode: BenchmarkHookMode::Disabled,
                peer_wire_security: BenchmarkPeerWireSecurity::PlaintextTest,
                direct_peer_listener: BenchmarkDirectPeerListener::Enabled,
            }
        ));

        for arguments in [
            vec!["--hook-mode", "disabled"],
            vec![
                "--hook-mode",
                "disabled",
                "--peer-wire-security",
                "plaintext",
            ],
            vec![
                "--hook-mode",
                "disabled",
                "--peer-wire-security",
                "mtls",
                "--direct-peer-listener",
                "implicit",
            ],
        ] {
            let error = parse_benchmark_invocation(arguments.into_iter().map(str::to_owned))
                .expect_err("implicit or invalid benchmark wire security must be rejected");
            assert!(
                error.message().contains("peer-wire-security")
                    || error.message().contains("direct-peer-listener"),
                "unexpected error: {error}"
            );
        }

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
