//! Retained v1.4 router compatibility evidence for the additive task route.

use atm_core::api::{HttpRouteKind, http_route_kind_for_version, http_route_surface_for_version};
use atm_core::error::AtmErrorCode;
use atm_core::protocol::HttpApiVersion;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct V1_4Message {
    message_id: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct V1_4ListResponse {
    messages: Vec<V1_4Message>,
}

#[test]
fn v1_4_router_keeps_every_pre_task_route() {
    let retained = HttpApiVersion::parse("1.4.0").expect("retained v1.4 fixture");

    let retained_routes: Vec<_> = http_route_surface_for_version(&retained)
        .map(|route| (route.method, route.path_template))
        .collect();
    assert_eq!(
        retained_routes,
        vec![
            ("GET", "/v1/atm/messages/search"),
            ("GET", "/v1/atm/messages"),
            ("POST", "/v1/atm/messages"),
            ("DELETE", "/v1/atm/messages"),
            ("POST", "/v1/atm/messages/inspect"),
            ("POST", "/v1/atm/messages/read"),
            ("GET", "/v1/atm/doctor"),
            ("POST", "/v1/atm/runtime/reload"),
            ("POST", "/v1/atm/compatibility"),
            ("POST", "/v1/atm/heartbeat"),
            ("POST", "/v1/atm/queue/get-next"),
            ("POST", "/v1/atm/graft/receiver/register"),
            ("POST", "/v1/atm/graft/receiver/unregister"),
            ("POST", "/v1/atm/graft/receiver/lookup"),
            ("POST", "/v1/atm/graft/receiver/refresh"),
        ]
    );
}

#[test]
fn v1_4_client_ignores_additive_task_fields_in_a_known_response() {
    let response = serde_json::json!({
        "messages": [{"message_id": "retained-message"}],
        "task": {"task_id": "new-additive-task-field"},
    });
    let decoded: V1_4ListResponse =
        serde_json::from_value(response).expect("v1.4 reader ignores additive fields");
    assert_eq!(
        decoded,
        V1_4ListResponse {
            messages: vec![V1_4Message {
                message_id: "retained-message".to_owned(),
            }],
        }
    );
}

#[test]
fn v1_4_router_rejects_task_route_before_request_decoding_or_mutation() {
    let retained = HttpApiVersion::parse("1.4.0").expect("retained v1.4 fixture");
    let error = http_route_kind_for_version(&retained, "POST", "/v1/atm/tasks")
        .expect_err("task route was introduced after the retained router");
    assert_eq!(error.code(), AtmErrorCode::ClientDaemonVersionIncompatible);
    assert!(error.message().contains("requires API 1.5.0"));

    assert_eq!(
        http_route_kind_for_version(&retained, "POST", "/v1/atm/messages")
            .expect("pre-AZ route resolves before body decoding"),
        HttpRouteKind::Write
    );
}
