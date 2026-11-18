use super::*;

impl ReaderPool {
    #[cfg(test)]
    pub(crate) async fn submit<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        self.submit_optional(Some(deadline), operation).await
    }

    pub(crate) async fn submit_optional<T, F>(
        &self,
        deadline: Option<Duration>,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        self.submit_with_class(deadline, None, operation).await
    }

    #[cfg(test)]
    pub(crate) async fn submit_tool<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        self.submit_tool_optional(Some(deadline), operation).await
    }

    pub(crate) async fn submit_tool_optional<T, F>(
        &self,
        deadline: Option<Duration>,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let permit = acquire_optional_tool_slot(&self.inner.tool_class_slots, deadline).await?;
        self.submit_with_class(deadline, Some(permit), operation)
            .await
    }

    async fn submit_with_class<T, F>(
        &self,
        deadline: Option<Duration>,
        tool_class_slot: Option<OwnedSemaphorePermit>,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let expires_at = deadline.map(deadline_at).transpose()?;
        self.submit_with_class_at(expires_at, deadline, tool_class_slot, operation)
            .await
    }

    async fn submit_with_class_at<T, F>(
        &self,
        expires_at: Option<Instant>,
        deadline: Option<Duration>,
        tool_class_slot: Option<OwnedSemaphorePermit>,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let reservation = self.reserve_worker()?;
        let request_id = RequestId::next(&self.inner.next_request);
        let (reply, response) = tokio::sync::oneshot::channel();
        let request = Request {
            id: request_id,
            queued_at: Instant::now(),
            deadline: expires_at,
            _tool_class_slot: tool_class_slot,
            run: Box::new(move |connection, target, disposition| {
                let result = match disposition {
                    RequestDisposition::Execute => operation(connection, target),
                    RequestDisposition::ExpiredInQueue => Err(deadline_expired(
                        "waiting in queue",
                        deadline.expect("unbounded requests are not expired in queue"),
                    )),
                    RequestDisposition::Rejected(error) => Err(error),
                };
                let _ = reply.send(result);
            }),
        };
        self.send_async_request(&reservation, request, expires_at, deadline)
            .await?;
        self.await_async_response(reservation, request_id, response, expires_at, deadline)
            .await
    }

    async fn send_async_request(
        &self,
        reservation: &WorkerReservation,
        request: Request,
        expires_at: Option<Instant>,
        deadline: Option<Duration>,
    ) -> Result<(), ReadLaneError> {
        self.inner
            .metrics
            .queue_depth
            .fetch_add(1, Ordering::Relaxed);
        let send = reservation.sender.send(WorkerMessage::Request(request));
        let send_result = match expires_at {
            Some(expires_at) => {
                tokio::time::timeout(expires_at.saturating_duration_since(Instant::now()), send)
                    .await
            }
            None => Ok(send.await),
        };
        match send_result {
            Err(_) => {
                self.inner
                    .metrics
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                self.inner
                    .metrics
                    .expired_in_queue
                    .fetch_add(1, Ordering::Relaxed);
                return Err(deadline_expired(
                    "waiting in queue",
                    deadline.expect("finite expiration has a budget"),
                ));
            }
            Ok(Err(_)) => {
                self.inner
                    .metrics
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                return Err(ReadLaneError::Unavailable {
                    message: "reader worker stopped".to_owned(),
                });
            }
            Ok(Ok(())) => {}
        }
        Ok(())
    }

    async fn await_async_response<T>(
        &self,
        reservation: WorkerReservation,
        request_id: RequestId,
        response: tokio::sync::oneshot::Receiver<Result<T, ReadLaneError>>,
        expires_at: Option<Instant>,
        deadline: Option<Duration>,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
    {
        let response = match expires_at {
            Some(expires_at) => match tokio::time::timeout(
                expires_at.saturating_duration_since(Instant::now()),
                response,
            )
            .await
            {
                Err(_) => {
                    return Err(self.expire_waiting_request(
                        reservation,
                        request_id,
                        deadline.expect("finite expiration has a budget"),
                    ));
                }
                Ok(response) => response,
            },
            None => response.await,
        };
        match response {
            Err(_) => Err(ReadLaneError::Unavailable {
                message: "reader worker closed its reply channel".to_owned(),
            }),
            Ok(result) => result,
        }
    }
}

async fn acquire_optional_tool_slot(
    semaphore: &Arc<tokio::sync::Semaphore>,
    deadline: Option<Duration>,
) -> Result<OwnedSemaphorePermit, ReadLaneError> {
    let permit = Arc::clone(semaphore).acquire_owned();
    let permit = match deadline {
        Some(deadline) => tokio::time::timeout(deadline, permit)
            .await
            .map_err(|_| deadline_expired("waiting for tool read capacity", deadline))?,
        None => permit.await,
    };
    permit.map_err(|_| ReadLaneError::Unavailable {
        message: "tool read capacity closed".to_owned(),
    })
}
