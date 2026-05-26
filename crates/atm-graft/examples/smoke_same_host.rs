use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use atm_core::ack::AckRequest;
use atm_core::read::ReadQuery;
use atm_core::send::{SendCommandOutcome, SendMessageSource, SendRequest};
use atm_core::types::{AckActivationMode, AgentName, ReadSelection, TeamName};
use atm_graft::{Event, GraftClient, GraftSession, GraftSessionOptions, HostNudgeInjector};
use serde_json::json;

#[derive(Debug)]
struct RecordingInjector {
    nudges: Mutex<Vec<Event>>,
    delivered_tx: mpsc::Sender<()>,
}

impl RecordingInjector {
    fn first_nudge(&self) -> Option<Event> {
        self.nudges.lock().expect("nudges lock").first().cloned()
    }

    fn count(&self) -> usize {
        self.nudges.lock().expect("nudges lock").len()
    }
}

impl HostNudgeInjector for RecordingInjector {
    fn inject_nudge(&self, nudge: Event) -> Result<(), atm_core::error::AtmError> {
        self.nudges.lock().expect("nudges lock").push(nudge);
        let _ = self.delivered_tx.send(());
        Ok(())
    }
}

struct Args {
    workspace_root: PathBuf,
    team: TeamName,
    agent: AgentName,
    reply_target: String,
    expected_nudge_substring: String,
    expected_sender: AgentName,
    ready_file: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut args = std::env::args().skip(1);
        let workspace_root = PathBuf::from(args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: smoke_same_host <workspace_root> <team> <agent> <reply_target> <expected_nudge_substring> <expected_sender> <ready_file>",
            )
        })?);
        let team: TeamName = args
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing team argument"))?
            .parse()?;
        let agent: AgentName = args
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing agent argument"))?
            .parse()?;
        let reply_target = args.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "missing reply_target argument")
        })?;
        let expected_nudge_substring = args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing expected_nudge_substring argument",
            )
        })?;
        let expected_sender: AgentName = args
            .next()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "missing expected_sender argument",
                )
            })?
            .parse()?;
        let ready_file = PathBuf::from(args.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "missing ready_file argument")
        })?);
        if args.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "too many arguments for smoke_same_host",
            )
            .into());
        }
        Ok(Self {
            workspace_root,
            team,
            agent,
            reply_target,
            expected_nudge_substring,
            expected_sender,
            ready_file,
        })
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse()?;
    let home_dir = PathBuf::from(
        std::env::var_os("ATM_HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ATM_HOME is not set"))?,
    );

    let client = GraftClient::connect()?;
    let (delivered_tx, delivered_rx) = mpsc::channel();
    let injector = Arc::new(RecordingInjector {
        nudges: Mutex::new(Vec::new()),
        delivered_tx,
    });
    let session = GraftSession::activate(
        client,
        GraftSessionOptions::for_current_process(
            &args.workspace_root,
            args.team.clone(),
            args.agent.clone(),
        ),
        Arc::clone(&injector) as Arc<dyn HostNudgeInjector>,
    )?;

    let activation_snapshot = session.snapshot()?;
    if activation_snapshot.state != atm_core::graft::AdvisorySessionState::Registered {
        return Err(io::Error::other(format!(
            "expected registered graft session, found {:?}",
            activation_snapshot.state
        ))
        .into());
    }

    std::fs::write(&args.ready_file, "ready\n")?;

    let nudge_timeout_secs = std::env::var("ATM_SMOKE_GRAFT_NUDGE_TIMEOUT_SECS")
        .unwrap_or_else(|_| "30".to_string())
        .parse::<u64>()?;
    delivered_rx
        .recv_timeout(Duration::from_secs(nudge_timeout_secs))
        .map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "timed out waiting for graft nudge")
        })?;
    let nudge = injector
        .first_nudge()
        .ok_or_else(|| io::Error::other("graft injector delivered no nudge payload"))?;
    if nudge.from != args.expected_sender {
        return Err(io::Error::other(format!(
            "expected nudge sender {}, found {}",
            args.expected_sender, nudge.from
        ))
        .into());
    }
    if !nudge
        .message
        .as_str()
        .contains(&args.expected_nudge_substring)
    {
        return Err(io::Error::other(format!(
            "expected nudge message to contain {:?}, found {:?}",
            args.expected_nudge_substring,
            nudge.message.as_str()
        ))
        .into());
    }

    let target_address = format!("{}@{}", args.agent, args.team);
    let nudge_message_id = nudge.message_id.to_string();
    let read_outcome = session.read(ReadQuery::new(
        home_dir.clone(),
        args.workspace_root.clone(),
        Some(args.agent.as_str()),
        Some(target_address.as_str()),
        Some(args.team.as_str()),
        ReadSelection::All,
        false,
        false,
        AckActivationMode::ReadOnly,
        Some(nudge_message_id.as_str()),
        None,
        None,
        None,
        None,
        None,
    )?)?;
    let read_selected_message_id = read_outcome
        .selected_message_id
        .ok_or_else(|| io::Error::other("graft read returned no selected_message_id"))?;
    if read_selected_message_id != nudge.message_id {
        return Err(io::Error::other(format!(
            "graft read selected unexpected message id {} instead of {}",
            read_selected_message_id, nudge.message_id
        ))
        .into());
    }

    let ack_outcome = session.ack(AckRequest {
        home_dir: home_dir.clone(),
        current_dir: args.workspace_root.clone(),
        actor_override: Some(args.agent.clone()),
        team_override: Some(args.team.clone()),
        message_id: nudge.message_id,
        reply_body: "graft smoke ack reply".to_string(),
    })?;

    let follow_up_outcome = session.send(SendRequest::new(
        home_dir,
        args.workspace_root.clone(),
        Some(args.agent.as_str()),
        args.reply_target.as_str(),
        Some(args.team.as_str()),
        SendMessageSource::Inline("graft smoke follow-up".to_string()),
        None,
        false,
        None,
        false,
    )?)?;
    if follow_up_outcome.outcome != SendCommandOutcome::Sent {
        return Err(io::Error::other(format!(
            "expected graft follow-up send outcome sent, found {:?}",
            follow_up_outcome.outcome
        ))
        .into());
    }

    session.close()?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "status": "passed",
            "session_state_before_close": activation_snapshot.state,
            "nudge_count": injector.count(),
            "nudge_message_id": nudge.message_id,
            "nudge_from": nudge.from,
            "nudge_text": nudge.message.as_str(),
            "read_selected_message_id": read_selected_message_id,
            "ack_message_id": ack_outcome.message_id,
            "ack_reply_message_id": ack_outcome.reply_message_id,
            "follow_up_message_id": follow_up_outcome.message_id,
        }))?
    );

    Ok(())
}
