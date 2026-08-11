use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use atm_core::boundary::PostSendHookEvent;
use atm_core::read::ReadQuery;
use atm_core::send::{SendCommandOutcome, SendMessageSource, SendRequest};
use atm_core::types::{AgentName, ReadSelection, TeamName};
use atm_graft::{
    GraftClient, GraftSession, GraftSessionOptions, GraftSessionState, HostNudge, HostNudgeInjector,
};
use serde_json::json;

#[derive(Debug)]
struct RecordingInjector {
    nudges: Mutex<Vec<PostSendHookEvent>>,
    delivered_tx: mpsc::Sender<()>,
}

impl RecordingInjector {
    fn first_nudge(&self) -> Option<PostSendHookEvent> {
        self.nudges.lock().expect("nudges lock").first().cloned()
    }

    fn count(&self) -> usize {
        self.nudges.lock().expect("nudges lock").len()
    }
}

impl HostNudgeInjector for RecordingInjector {
    fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), atm_core::error::AtmError> {
        self.nudges
            .lock()
            .expect("nudges lock")
            .push(nudge.event.clone());
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
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
        GraftSessionOptions::new(&args.workspace_root, args.team.clone(), args.agent.clone()),
        Arc::clone(&injector) as Arc<dyn HostNudgeInjector>,
    )?;

    let activation_snapshot = session.snapshot()?;
    if activation_snapshot.state != GraftSessionState::Listening {
        return Err(io::Error::other(format!(
            "expected listening graft session, found {:?}",
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
    if nudge.sender != args.expected_sender {
        return Err(io::Error::other(format!(
            "expected nudge sender {}, found {}",
            args.expected_sender, nudge.sender
        ))
        .into());
    }
    if !nudge
        .description
        .as_str()
        .contains(&args.expected_nudge_substring)
    {
        return Err(io::Error::other(format!(
            "expected nudge description to contain {:?}, found {:?}",
            args.expected_nudge_substring,
            nudge.description.as_str()
        ))
        .into());
    }

    let target_address = format!("{}@{}", args.agent, args.team);
    let nudge_message_id = nudge.message_id.to_string();
    let read_outcome = session
        .read(ReadQuery::new(
            home_dir.clone(),
            args.workspace_root.clone(),
            args.agent.parse().expect("caller"),
            Some(target_address.as_str()),
            args.team.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            Some(nudge_message_id.as_str()),
            None,
            None,
            None,
            None,
            None,
        )?)
        .await?;
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

    let follow_up_outcome = session
        .send(SendRequest::new(
            home_dir,
            args.workspace_root.clone(),
            args.agent.parse().expect("caller"),
            args.reply_target.as_str(),
            args.team.parse().expect("team"),
            SendMessageSource::Inline("graft smoke follow-up".to_string()),
            None,
            false,
            None,
            false,
        )?)
        .await?;
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
            "session_state_before_close": format!("{:?}", activation_snapshot.state),
            "nudge_count": injector.count(),
            "injected_nudge": {
                "sender": nudge.sender.to_string(),
                "sender_team": nudge.sender_team.to_string(),
                "recipient": nudge.recipient.to_string(),
                "recipient_team": nudge.recipient_team.to_string(),
                "message_id": nudge.message_id.to_string(),
                "description": nudge.description,
                "requires_ack": nudge.requires_ack,
                "is_ack": nudge.is_ack,
                "task_id": nudge.task_id.map(|task_id| task_id.to_string()),
            },
            "read_selected_message_id": read_selected_message_id.to_string(),
            "follow_up_message_id": follow_up_outcome.message_id.to_string(),
        }))?
    );

    Ok(())
}
