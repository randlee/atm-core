use atm_core::boundary;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::ResponseEnvelope;
use atm_core::schema::AtmMessageId;

#[derive(Debug)]
pub(crate) enum TransportAttemptResult {
    ImmediateResponse(Box<ResponseEnvelope>),
    ImmediateError(AtmError),
    ListenerUnavailable(AtmError),
}

#[derive(Debug)]
pub(crate) enum OutboundDeliveryDisposition {
    Delivered(Box<ResponseEnvelope>),
    Deferred {
        receipt_message_id: AtmMessageId,
        error: AtmError,
    },
    RejectedTerminal(AtmError),
    OutcomeUnknown {
        receipt_message_id: AtmMessageId,
        error: AtmError,
    },
}

pub(crate) fn shared_outcome_policy(
    attempt: TransportAttemptResult,
    receipt_message_id: AtmMessageId,
) -> OutboundDeliveryDisposition {
    match attempt {
        TransportAttemptResult::ImmediateResponse(response) => {
            OutboundDeliveryDisposition::Delivered(response)
        }
        TransportAttemptResult::ListenerUnavailable(error) => {
            OutboundDeliveryDisposition::Deferred {
                receipt_message_id,
                error: preserve_error_shape(&error),
            }
        }
        TransportAttemptResult::ImmediateError(error) => classify_error(error, receipt_message_id),
    }
}

pub(crate) fn requires_replay(disposition: &OutboundDeliveryDisposition) -> bool {
    matches!(
        disposition,
        OutboundDeliveryDisposition::Deferred { .. }
            | OutboundDeliveryDisposition::OutcomeUnknown { .. }
    )
}

pub(crate) fn into_remote_send_delivery_outcome(
    disposition: OutboundDeliveryDisposition,
) -> boundary::RemoteSendDeliveryOutcome {
    match disposition {
        OutboundDeliveryDisposition::Delivered(response) => {
            boundary::RemoteSendDeliveryOutcome::Delivered(response)
        }
        OutboundDeliveryDisposition::Deferred {
            receipt_message_id,
            error,
        } => boundary::RemoteSendDeliveryOutcome::Deferred {
            receipt_message_id,
            error,
        },
        OutboundDeliveryDisposition::RejectedTerminal(error) => {
            boundary::RemoteSendDeliveryOutcome::RejectedTerminal(error)
        }
        OutboundDeliveryDisposition::OutcomeUnknown {
            receipt_message_id,
            error,
        } => boundary::RemoteSendDeliveryOutcome::OutcomeUnknown {
            receipt_message_id,
            error,
        },
    }
}

fn classify_error(
    error: AtmError,
    receipt_message_id: AtmMessageId,
) -> OutboundDeliveryDisposition {
    if error.code == AtmErrorCode::RemoteDeliveryOutcomeUnknown {
        return OutboundDeliveryDisposition::OutcomeUnknown {
            receipt_message_id,
            error: preserve_error_shape(&error),
        };
    }
    if error.code == AtmErrorCode::ClientDaemonVersionIncompatible {
        return OutboundDeliveryDisposition::RejectedTerminal(
            AtmError::validation("remote daemon rejected the cross-host request protocol")
                .with_recovery(
                    "Align the sender and receiver daemon protocol versions before retrying.",
                ),
        );
    }
    if error.code == AtmErrorCode::AddressParseFailed || error.is_validation() {
        let mut terminal = AtmError::new_with_code(error.code, error.kind, error.message.clone());
        for recovery in &error.recovery {
            terminal = terminal.with_recovery(recovery.clone());
        }
        return OutboundDeliveryDisposition::RejectedTerminal(terminal);
    }
    OutboundDeliveryDisposition::Deferred {
        receipt_message_id,
        error: preserve_error_shape(&error),
    }
}

fn preserve_error_shape(error: &AtmError) -> AtmError {
    let mut preserved = AtmError::new_with_code(error.code, error.kind, error.message.clone());
    for recovery in &error.recovery {
        preserved = preserved.with_recovery(recovery.clone());
    }
    preserved
}
