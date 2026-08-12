//! Bounded backend-owned reader lane for typed search.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use atm_storage::{AtmError, MessageSearchPage, MessageSearchQuery};

use crate::search_store::execute_search;
use crate::shared_db::{SharedDbTarget, open_connection_for_target};

const READER_CAPACITY: usize = 64;
const READER_DEADLINE: Duration = Duration::from_secs(10);

enum Reply {
    Sync(SyncSender<Result<MessageSearchPage, AtmError>>),
    Async(tokio::sync::oneshot::Sender<Result<MessageSearchPage, AtmError>>),
}

impl Reply {
    fn send(self, result: Result<MessageSearchPage, AtmError>) {
        match self {
            Self::Sync(sender) => {
                let _ = sender.send(result);
            }
            Self::Async(sender) => {
                let _ = sender.send(result);
            }
        }
    }
}

struct Request {
    query: MessageSearchQuery,
    reply: Reply,
}

pub(crate) struct SearchReader {
    sender: tokio::sync::mpsc::Sender<Request>,
}

impl std::fmt::Debug for SearchReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchReader")
            .finish_non_exhaustive()
    }
}

impl SearchReader {
    pub(crate) fn start(target: Arc<SharedDbTarget>) -> Result<Self, AtmError> {
        let connection = open_connection_for_target(target.as_ref())?;
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Request>(READER_CAPACITY);
        thread::Builder::new()
            .name("atm-sqlite-search-reader".to_owned())
            .spawn(move || {
                while let Some(request) = receiver.blocking_recv() {
                    request.reply.send(execute_search(
                        &request.query,
                        &connection,
                        target.as_ref(),
                    ));
                }
            })
            .map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to start SQLite search reader lane: {error}"
                ))
            })?;
        Ok(Self { sender })
    }

    pub(crate) fn submit(&self, query: MessageSearchQuery) -> Result<MessageSearchPage, AtmError> {
        let (reply, response) = mpsc::sync_channel(1);
        let deadline = Instant::now() + READER_DEADLINE;
        let mut request = Request {
            query,
            reply: Reply::Sync(reply),
        };
        loop {
            match self.sender.try_send(request) {
                Ok(()) => break,
                Err(tokio::sync::mpsc::error::TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        return Err(AtmError::daemon_unavailable(
                            "bounded SQLite search reader queue did not accept the request before its deadline",
                        ));
                    }
                    request = returned;
                    thread::park_timeout(Duration::from_millis(5));
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    return Err(AtmError::daemon_unavailable(
                        "SQLite search reader lane is unavailable",
                    ));
                }
            }
        }
        response
            .recv_timeout(READER_DEADLINE)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    AtmError::daemon_unavailable("SQLite search reader exceeded its deadline")
                }
                RecvTimeoutError::Disconnected => {
                    AtmError::daemon_unavailable("SQLite search reader reply channel closed")
                }
            })?
    }

    pub(crate) async fn submit_async(
        &self,
        query: MessageSearchQuery,
    ) -> Result<MessageSearchPage, AtmError> {
        let (reply, response) = tokio::sync::oneshot::channel();
        tokio::time::timeout(
            READER_DEADLINE,
            self.sender.send(Request {
                query,
                reply: Reply::Async(reply),
            }),
        )
        .await
        .map_err(|_| {
            AtmError::daemon_unavailable(
                "bounded SQLite search reader queue did not accept the request before its deadline",
            )
        })?
        .map_err(|_| AtmError::daemon_unavailable("SQLite search reader lane is unavailable"))?;
        tokio::time::timeout(READER_DEADLINE, response)
            .await
            .map_err(|_| {
                AtmError::daemon_unavailable("SQLite search reader exceeded its deadline")
            })?
            .map_err(|_| {
                AtmError::daemon_unavailable("SQLite search reader reply channel closed")
            })?
    }
}
