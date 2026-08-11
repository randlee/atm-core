//! Routing metadata derived from a canonical mailbox write.

use crate::address::AgentAddress;
use crate::types::HostName;

use super::WriteRequest;

/// Returns the direct-peer destination for a locally originated canonical write.
///
/// Inbound peer receipts and already-originated records never become another
/// outbound delivery. The destination is routing metadata only; it carries no
/// deferred request body, queue, retry, or replay state.
pub(crate) fn direct_peer_destination(
    request: &WriteRequest,
    destination: &AgentAddress,
) -> Option<HostName> {
    if request.authenticated_source_host.is_some() || request.origin_message_id.is_some() {
        return None;
    }
    destination.host().cloned()
}
