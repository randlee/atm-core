use atm_core::error::AtmError;
use atm_daemon_client::graft_rpc::{AdvisoryStreamRequest, RequestEnvelope, ResponseEnvelope};

pub(crate) trait GraftRequestDispatcher: Send + Sync {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the unary graft request cannot be executed
    /// or the dispatcher cannot encode a valid typed response envelope.
    fn dispatch_graft(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;

    /// # Errors
    ///
    /// Returns [`AtmError`] when the advisory-stream request cannot be served
    /// or streaming the response batches through the sink fails.
    fn dispatch_graft_stream(
        &self,
        request: AdvisoryStreamRequest,
        sink: &mut dyn GraftStreamSink,
    ) -> Result<(), AtmError>;
}

pub(crate) trait GraftStreamSink {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the sink cannot accept the next streamed
    /// graft response envelope.
    fn emit(&mut self, response: ResponseEnvelope) -> Result<(), AtmError>;

    fn stop_requested(&self) -> bool {
        false
    }
}
