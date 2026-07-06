use atm_core::error::AtmError;
use atm_daemon_client::graft_rpc::{AdvisoryStreamRequest, RequestEnvelope, ResponseEnvelope};

pub(crate) trait GraftRequestDispatcher: Send + Sync {
    fn dispatch_graft(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;

    fn dispatch_graft_stream(
        &self,
        request: AdvisoryStreamRequest,
        sink: &mut dyn GraftStreamSink,
    ) -> Result<(), AtmError>;
}

pub(crate) trait GraftStreamSink {
    fn emit(&mut self, response: ResponseEnvelope) -> Result<(), AtmError>;

    fn stop_requested(&self) -> bool {
        false
    }
}
