//! Historical post-commit work boundary.
//!
//! AL.3 performs receiver-hook delivery synchronously after the durable write
//! through `runtime_health::peer_delivery_router`. The former detached local
//! notification worker was removed with AK.2; this file remains as a ledger
//! anchor for the boundary test and must not regain background work.
