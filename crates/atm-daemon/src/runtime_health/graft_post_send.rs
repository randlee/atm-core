use std::io::Write as _;
use std::path::Path;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::thread;
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::{self, GraftNudgeTarget, PostSendHookEvent};
use atm_core::error::{AtmError, AtmErrorKind};
use atm_core::error_codes::AtmErrorCode;
use atm_core::graft::{
    GraftPostSendRequest, GraftPostSendResponse, graft_receiver_socket_path_from_home,
    read_graft_post_send_message, write_graft_post_send_message,
};
use atm_core::schema::canonical_home_dir;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

const GRAFT_POST_SEND_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_POST_SEND_IO_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(crate) struct DaemonGraftPostSendPort {
    runtime: LocalServiceRuntime,
}

impl DaemonGraftPostSendPort {
    pub(crate) fn new(runtime: LocalServiceRuntime) -> Self {
        Self { runtime }
    }
}

impl boundary::sealed::Sealed for DaemonGraftPostSendPort {}

impl boundary::GraftPostSendPort for DaemonGraftPostSendPort {
    fn deliver_post_send(
        &self,
        event: &PostSendHookEvent,
        target: &GraftNudgeTarget,
    ) -> Result<(), AtmError> {
        let Some(member) = self
            .runtime
            .load_roster_member(&target.recipient_team, &target.recipient)?
        else {
            return Err(graft_recipient_unavailable_error(
                event,
                "recipient is missing from the authoritative ATM roster",
            )
            .with_recovery(
                "Repair the roster row and restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ));
        };
        let recipient_home_dir = canonical_home_dir(&member.metadata_json).ok_or_else(|| {
            graft_recipient_unavailable_error(
                event,
                "recipient has no authoritative home_dir for graft post-send delivery",
            )
            .with_recovery(format!(
                "Repair the roster row with `atm teams update-member --team {} --member {} --home-dir <path>` and restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
                target.recipient_team, target.recipient
            ))
        })?;
        let endpoint_path = graft_receiver_socket_path_from_home(
            recipient_home_dir.as_path(),
            &target.recipient_team,
            &target.recipient,
        );
        deliver_post_send_to_graft_receiver(&endpoint_path, event)
    }
}

fn deliver_post_send_to_graft_receiver(
    endpoint_path: &Path,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let mut stream = connect_graft_receiver(endpoint_path, event)?;
    apply_graft_post_send_deadlines(&stream, event)?;
    let request = GraftPostSendRequest {
        event: event.clone(),
    };
    write_graft_post_send_message(
        &mut stream,
        &request,
        "failed to write graft post-send request",
        "graft post-send request exceeded the bounded payload cap",
    )
    .map_err(|error| graft_transport_error(event, error))?;
    stream
        .flush()
        .map_err(|source| graft_transport_error(event, AtmError::daemon_unavailable(
            "failed to flush graft post-send request",
        )
        .with_recovery(
            "Restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
        )
        .with_source(source)))?;
    match read_graft_post_send_message::<GraftPostSendResponse>(
        &mut stream,
        "failed to read graft post-send response",
        "graft post-send response exceeded the bounded payload cap",
    )
    .map_err(|error| graft_transport_error(event, error))?
    {
        GraftPostSendResponse::Delivered => Ok(()),
        GraftPostSendResponse::Error(error) => Err(error.into_atm_error()),
    }
}

fn connect_graft_receiver(
    endpoint_path: &Path,
    event: &PostSendHookEvent,
) -> Result<LocalSocketStream, AtmError> {
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let (result_tx, result_rx) = sync_channel(1);
    thread::Builder::new()
        .name("graft-post-send-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(name));
        })
        .map_err(|source| {
            graft_recipient_unavailable_error(
                event,
                "failed to spawn bounded graft post-send connect helper",
            )
            .with_recovery(
                "Retry after the daemon can spawn one bounded same-host connect helper thread.",
            )
            .with_source(source)
        })?;
    match result_rx.recv_timeout(GRAFT_POST_SEND_CONNECT_DEADLINE) {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(source)) => Err(
            graft_recipient_unavailable_error(event, "recipient has no active graft receiver path")
                .with_recovery(
                    "Start or reconnect the graft-backed recipient session before retrying if a fresh nudge is still required.",
                )
                .with_source(source),
        ),
        Err(RecvTimeoutError::Timeout) => Err(
            graft_recipient_unavailable_error(
                event,
                "timed out connecting to the graft receiver path",
            )
            .with_recovery(
                "Start or reconnect the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ),
        ),
        Err(RecvTimeoutError::Disconnected) => Err(
            graft_recipient_unavailable_error(
                event,
                "graft post-send connect helper disconnected unexpectedly",
            )
            .with_recovery(
                "Restart the daemon and the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ),
        ),
    }
}

fn apply_graft_post_send_deadlines(
    stream: &LocalSocketStream,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    apply_graft_post_send_deadline(
        stream.set_recv_timeout(Some(GRAFT_POST_SEND_IO_DEADLINE)),
        event,
        "failed to apply graft post-send receive timeout",
    )?;
    apply_graft_post_send_deadline(
        stream.set_send_timeout(Some(GRAFT_POST_SEND_IO_DEADLINE)),
        event,
        "failed to apply graft post-send send timeout",
    )
}

fn apply_graft_post_send_deadline(
    result: std::io::Result<()>,
    event: &PostSendHookEvent,
    message: &'static str,
) -> Result<(), AtmError> {
    match result {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => Ok(()),
        Err(source) => Err(
            graft_recipient_unavailable_error(event, message)
                .with_recovery(
                    "Restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
                )
                .with_source(source),
        ),
    }
}

fn graft_transport_error(event: &PostSendHookEvent, error: AtmError) -> AtmError {
    let mut graft_error = graft_recipient_unavailable_error(event, &error.message);
    for recovery in error.recovery {
        graft_error = graft_error.with_recovery(recovery);
    }
    graft_error
}

fn graft_recipient_unavailable_error(
    event: &PostSendHookEvent,
    message: impl Into<String>,
) -> AtmError {
    AtmError::new_with_code(
        AtmErrorCode::PostSendGraftUnavailable,
        AtmErrorKind::DaemonUnavailable,
        format!(
            "recipient {}@{} {}",
            event.recipient,
            event.recipient_team,
            message.into()
        ),
    )
}
