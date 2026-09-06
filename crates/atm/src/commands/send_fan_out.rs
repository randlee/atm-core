//! `atm send --from-json` fan-out orchestration (ADR-055 decisions (e)-(g)).
//!
//! Split out of `commands/send.rs` (RULE-003: no file exceeding 1000 lines
//! of non-test code) as a sibling module. `SendCommand`'s fan-out entry
//! point ([`SendCommand::run_from_json`]) and its per-recipient send/staging
//! helpers live here; the fields and methods they need from `SendCommand`
//! are `pub(super)` in `send.rs`, visible throughout `commands` and its
//! descendants (this module included).

use std::path::PathBuf;

use anyhow::{Context, Result};
use atm_core::address::AgentAddress;
use atm_core::load_atm_config;
use atm_core::send::{NudgeMode, SendMessageSource, SendRequest};
use atm_core::send_to::{PickerOutput, RecipientLocality, classify_recipient_locality};
use atm_core::types::HostName;
use atm_daemon_bootstrap::with_default_peer_address_stores;

use crate::commands::send::{SendCommand, combine_message_with_attachment_note};
use crate::commands::send_to::{land_attachments, resolve_atm_temp_for_cli};
use crate::composition::{AtmHomePath, CliComposition, InvocationDir};
use crate::observability::CliObservability;

/// One `--from-json` recipient, resolved and classified before any staging,
/// transfer, or send begins (R5/R13). `pub(crate)`: constructed directly by
/// `send.rs`'s fan-out integration test.
pub(crate) struct FanOutRecipient {
    pub(crate) member_id: String,
    pub(crate) address: AgentAddress,
    pub(crate) locality: RecipientLocality,
}

impl SendCommand {
    /// Executes the `--from-json` fan-out surface (ADR-055 decisions (e)-
    /// (g)): reads one `PickerOutput` document from stdin, resolves and
    /// validates every recipient and attachment path up front (R5/R13), then
    /// sends one immutable message per recipient in array order.
    ///
    /// On a transfer or send failure for recipient N (decision (g)), aborts
    /// every remaining not-yet-attempted recipient -- no further transfer or
    /// send calls -- and reports a partial delivered/not-delivered result by
    /// recipient id on stderr and in `--json` output, then returns an error
    /// (nonzero exit).
    pub(super) async fn run_from_json(
        self,
        observability: &CliObservability,
        nudge_mode: NudgeMode,
        command_name: &'static str,
        home_dir: PathBuf,
        current_dir: PathBuf,
    ) -> Result<()> {
        let picker_output = read_picker_output_from_stdin()?;
        let caller_context = self.resolve_caller_context()?;
        let local_host = load_atm_config(&current_dir)?.and_then(|config| config.local_host);

        // Resolve every recipient and validate every attachment source
        // before any staging, transfer, or send begins (R5/R13): a
        // malformed request stages and sends nothing.
        let recipients = resolve_fan_out_recipients(&picker_output, local_host.as_ref())?;
        validate_attach_sources(&self.attach)?;

        let atm_temp = if self.attach.is_empty() {
            None
        } else {
            Some(resolve_atm_temp_for_cli()?)
        };
        let composition = CliComposition::bootstrap(
            command_name,
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;

        let (delivered, not_delivered, failure) = self
            .send_fan_out_recipients(
                &recipients,
                &picker_output,
                &composition,
                &current_dir,
                home_dir,
                nudge_mode,
                atm_temp.as_ref(),
                &caller_context,
            )
            .await;

        report_fan_out_result(self.json, &delivered, &not_delivered);
        failure.map_or(Ok(()), Err)
    }

    /// Sends to every resolved recipient in array order (decision (g)):
    /// aborts remaining recipients on the first transfer or send failure, no
    /// further transfer or send calls. Returns the delivered/not-delivered
    /// recipient-id lists and, on abort, the triggering error.
    #[allow(
        clippy::too_many_arguments,
        reason = "fan-out send threads through shared, already-resolved batch state"
    )]
    pub(super) async fn send_fan_out_recipients(
        &self,
        recipients: &[FanOutRecipient],
        picker_output: &PickerOutput,
        composition: &CliComposition<'_>,
        current_dir: &std::path::Path,
        home_dir: PathBuf,
        nudge_mode: NudgeMode,
        atm_temp: Option<&atm_core::atm_temp::AtmTemp>,
        caller_context: &atm_core::caller_context::CallerContext,
    ) -> (Vec<String>, Vec<String>, Option<anyhow::Error>) {
        let mut landed_by_locality: Vec<(RecipientLocality, PathBuf)> = Vec::new();
        let mut delivered: Vec<String> = Vec::new();
        let mut not_delivered: Vec<String> = Vec::new();
        let mut failure: Option<anyhow::Error> = None;

        for recipient in recipients {
            if failure.is_some() {
                not_delivered.push(recipient.member_id.clone());
                continue;
            }
            match self
                .send_one_fan_out_recipient(
                    recipient,
                    picker_output,
                    composition,
                    current_dir,
                    home_dir.clone(),
                    nudge_mode,
                    atm_temp,
                    &mut landed_by_locality,
                    caller_context,
                )
                .await
            {
                Ok(()) => delivered.push(recipient.member_id.clone()),
                Err(error) => {
                    not_delivered.push(recipient.member_id.clone());
                    failure = Some(error);
                }
            }
        }

        (delivered, not_delivered, failure)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "fan-out per-recipient send threads through shared, already-resolved batch state"
    )]
    async fn send_one_fan_out_recipient(
        &self,
        recipient: &FanOutRecipient,
        picker_output: &PickerOutput,
        composition: &CliComposition<'_>,
        current_dir: &std::path::Path,
        home_dir: PathBuf,
        nudge_mode: NudgeMode,
        atm_temp: Option<&atm_core::atm_temp::AtmTemp>,
        landed_by_locality: &mut Vec<(RecipientLocality, PathBuf)>,
        caller_context: &atm_core::caller_context::CallerContext,
    ) -> Result<()> {
        let attachment_note = if self.attach.is_empty() {
            None
        } else {
            let atm_temp = atm_temp.expect("atm_temp is resolved whenever --attach is non-empty");
            let landed_dir = match landed_by_locality
                .iter()
                .find(|(locality, _)| *locality == recipient.locality)
            {
                Some((_, landed_dir)) => landed_dir.clone(),
                None => {
                    let landing = land_attachments(
                        atm_temp,
                        ulid::Ulid::new(),
                        &recipient.locality,
                        &self.attach,
                    )
                    .await?;
                    landed_by_locality
                        .push((recipient.locality.clone(), landing.landed_dir.clone()));
                    landing.landed_dir
                }
            };
            Some(atm_core::send_to::format_attachment_note(
                &landed_dir,
                &self.attach,
            ))
        };
        let message_text = combine_message_with_attachment_note(
            picker_output.note.clone(),
            attachment_note.as_deref(),
        )
        .unwrap_or_default();

        let request = SendRequest::new(
            home_dir,
            current_dir.to_path_buf(),
            caller_context.caller_identity.clone(),
            &recipient.address.to_string(),
            caller_context.caller_team.clone(),
            SendMessageSource::Inline(message_text),
            None,
            self.requires_ack,
            self.task_id.clone(),
            self.dry_run,
        )?
        .with_task_complete(self.task_complete.clone())
        .with_caller_chat_id(caller_context.caller_chat_id.clone())
        .with_activity_observation(caller_context.activity_observation.clone())
        .with_nudge_mode(nudge_mode);

        composition.send(request).await?;
        Ok(())
    }
}

fn read_picker_output_from_stdin() -> Result<PickerOutput> {
    use std::io::Read as _;
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .map_err(|error| anyhow::anyhow!("--from-json stdin could not be read: {error}"))?;
    atm_core::send_to::parse_picker_output(&raw)
        .map_err(atm_core::error::AtmError::from)
        .map_err(Into::into)
}

/// Resolves and classifies every `--from-json` recipient against the roster
/// before any staging, transfer, or send begins (R5/R13): an
/// unknown/null-host/unclassifiable recipient anywhere in the batch fails
/// the whole invocation closed.
fn resolve_fan_out_recipients(
    picker_output: &PickerOutput,
    local_host: Option<&HostName>,
) -> Result<Vec<FanOutRecipient>> {
    let mut recipients = Vec::with_capacity(picker_output.recipients.len());
    for member_id in &picker_output.recipients {
        // RBQA-F001: resolves through the canonical, non-deprecated
        // `atm_storage::RosterStore` -- the same accessor `send.rs`'s
        // peer-authority target resolution uses -- not the deprecated
        // `atm_core::boundary::RosterStore`.
        let address = with_default_peer_address_stores(|roster, _peer| {
            atm_core::send_to::resolve_picker_recipient(member_id, roster)
                .map_err(atm_core::error::AtmError::from)
        })?;
        let locality = classify_recipient_locality(address.host(), local_host)
            .map_err(atm_core::error::AtmError::from)?;
        recipients.push(FanOutRecipient {
            member_id: member_id.clone(),
            address,
            locality,
        });
    }
    Ok(recipients)
}

/// Validates every `--attach` source file's readability once, up front,
/// across the whole `--from-json` batch (R5/R13): a missing/unreadable
/// source must fail before any per-host staging or transfer for *any*
/// recipient.
fn validate_attach_sources(files: &[PathBuf]) -> Result<()> {
    for file in files {
        std::fs::metadata(file)
            .with_context(|| format!("attachment '{}' could not be read", file.display()))?;
    }
    Ok(())
}

/// Reports the decision-(g) partial delivery result on stderr, and includes
/// it in `--json` output when requested.
fn report_fan_out_result(json: bool, delivered: &[String], not_delivered: &[String]) {
    if json {
        eprintln!("{}", fan_out_result_json(delivered, not_delivered));
    } else {
        eprintln!("delivered: {}", delivered.join(", "));
        if !not_delivered.is_empty() {
            eprintln!("not delivered: {}", not_delivered.join(", "));
        }
    }
}

/// Builds the decision-(g) `--json` partial-delivery report. Extracted from
/// [`report_fan_out_result`] so its exact shape is unit-testable without
/// capturing process stderr. `pub(crate)`: also exercised directly by
/// `send.rs`'s test module.
pub(crate) fn fan_out_result_json(
    delivered: &[String],
    not_delivered: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "delivered": delivered,
        "not_delivered": not_delivered,
    })
}
