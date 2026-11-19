//! Commit notifications shared by mailbox readers.

use std::sync::{Arc, Mutex};

/// Fans one backend-owned commit stream out to concurrent mailbox readers.
#[derive(Clone)]
pub(super) struct MailboxCommitFanout {
    inner: Arc<MailboxCommitFanoutInner>,
}

struct MailboxCommitFanoutInner {
    source: Mutex<Option<Box<dyn atm_storage::MailboxCommitWatch>>>,
    generation_tx: tokio::sync::watch::Sender<u64>,
    generation_rx: tokio::sync::watch::Receiver<u64>,
    started: std::sync::atomic::AtomicBool,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Drop for MailboxCommitFanoutInner {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

impl MailboxCommitFanout {
    pub(super) fn new(source: Box<dyn atm_storage::MailboxCommitWatch>) -> Self {
        let (generation_tx, generation_rx) = tokio::sync::watch::channel(0_u64);
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self {
            inner: Arc::new(MailboxCommitFanoutInner {
                source: Mutex::new(Some(source)),
                generation_tx,
                generation_rx,
                started: std::sync::atomic::AtomicBool::new(false),
                shutdown,
            }),
        }
    }

    pub(super) fn start(&self) {
        if self
            .inner
            .started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let Some(mut source) = self
            .inner
            .source
            .lock()
            .expect("mailbox commit subscription mutex poisoned")
            .take()
        else {
            return;
        };
        let generation_tx = self.inner.generation_tx.clone();
        let mut shutdown = self.inner.shutdown.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = source.changed() => {
                        if !changed {
                            break;
                        }
                        generation_tx.send_modify(|generation| {
                            *generation = generation.wrapping_add(1);
                        });
                    }
                    result = shutdown.changed() => {
                        if result.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        });
    }

    pub(super) fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.inner.generation_rx.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MailboxWaitOutcome {
    Commit,
    WaitExpired,
    RequestExpired,
}

pub(super) async fn wait_for_mail_commit(
    commit_wake: &mut tokio::sync::watch::Receiver<u64>,
    wait_deadline: tokio::time::Instant,
    request_deadline: tokio::time::Instant,
) -> MailboxWaitOutcome {
    let wake_deadline = wait_deadline.min(request_deadline);
    let result = tokio::time::timeout_at(wake_deadline, commit_wake.changed()).await;
    if matches!(&result, Ok(Ok(()))) && tokio::time::Instant::now() < wake_deadline {
        return MailboxWaitOutcome::Commit;
    }
    if matches!(&result, Ok(Err(_))) {
        tokio::time::timeout_at(wake_deadline, std::future::pending::<()>())
            .await
            .ok();
    }
    if wait_deadline <= request_deadline {
        MailboxWaitOutcome::WaitExpired
    } else {
        MailboxWaitOutcome::RequestExpired
    }
}
