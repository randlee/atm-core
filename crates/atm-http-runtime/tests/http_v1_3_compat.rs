//! Retained v1.3 consumer compatibility evidence for the additive task route.

use atm_core::api::endpoint_for;
use atm_core::protocol::{HttpApiVersion, RequestEnvelope};

#[test]
fn v1_3_consumer_keeps_its_pre_task_route_and_same_major_compatibility() {
    let client = HttpApiVersion::parse("1.3.0").expect("retained v1.3 fixture");
    let server = HttpApiVersion::current();
    assert_eq!(client.major(), server.major());

    let request = RequestEnvelope::ReloadRuntimeView;
    let (method, path) = endpoint_for(&request);
    assert_eq!((method, path.as_str()), ("POST", "/v1/atm/runtime/reload"));
}
