//! Small writer shutdown and reply helpers kept outside the lane implementation.

use std::time::Duration;

use crate::observability::{
    SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome,
};
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::AtmError;

use super::{WriterMessage, WriterMessage::Submit};

pub(super) fn writer_channel_closed_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer submission channel closed")
}

pub(super) fn writer_queue_timeout_error(deadline: Duration) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite writer submission queue did not accept a write within {:?}",
        deadline
    ))
}

pub(super) fn writer_reply_timeout_error(deadline: Duration) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite writer reply did not arrive within {:?}",
        deadline
    ))
}

pub(super) fn writer_reply_channel_closed_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer reply channel closed")
}

pub(super) fn writer_unavailable_reply_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer is unavailable during shutdown")
}

pub(super) fn drain_submit_replies(receiver: &mut tokio::sync::mpsc::Receiver<WriterMessage>) {
    loop {
        match receiver.try_recv() {
            Ok(Submit { reply, .. }) => reply.send(Err(writer_unavailable_reply_error())),
            Ok(WriterMessage::Shutdown) => continue,
            Err(
                tokio::sync::mpsc::error::TryRecvError::Empty
                | tokio::sync::mpsc::error::TryRecvError::Disconnected,
            ) => break,
        }
    }
}

pub(super) fn checkpoint_writer_connection(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    observability: &dyn SqliteObservability,
) {
    #[cfg(test)]
    if matches!(target, SharedDbTarget::InMemory { .. }) {
        return;
    }

    if let Err(error) = connection.query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |_row| Ok(())) {
        let error = sqlite_error(
            target,
            "sqlite writer final wal checkpoint failed after draining the write lane",
            error,
        );
        tracing::warn!(
            path = %target.display(),
            %error,
            "sqlite writer final wal checkpoint failed after draining the write lane"
        );
        observability.emit_or_warn(SqliteObservabilityEvent::new(
            "writer_shutdown_checkpoint",
            SqliteObservabilityOutcome::Failed,
            error.message().to_owned(),
            Some(error.code()),
        ));
    }
}
