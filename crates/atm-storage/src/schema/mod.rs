mod inbox_message;

pub use inbox_message::MessageEnvelope as InboxMessage;
pub use inbox_message::{AlertKind, AtmMessageId, MessageEnvelope, PendingAck, ThreadMode};
