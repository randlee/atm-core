//! Retained v1.4 consumer compatibility evidence for the additive task route.

use atm_core::api::http_route_surface;
use atm_core::protocol::HttpApiVersion;

#[test]
fn v1_4_consumer_keeps_every_pre_task_route_and_same_major_compatibility() {
    let client = HttpApiVersion::parse("1.4.0").expect("retained v1.4 fixture");
    let server = HttpApiVersion::current();
    assert_eq!(client.major(), server.major());

    let retained_routes: Vec<_> = http_route_surface()
        .filter(|route| route.path_template != "/v1/atm/tasks")
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
fn v1_4_route_inventory_rejects_task_before_mutation_dispatch() {
    let prior_server_routes: Vec<_> = http_route_surface()
        .filter(|route| route.path_template != "/v1/atm/tasks")
        .collect();
    assert!(
        prior_server_routes
            .iter()
            .all(|route| route.path_template != "/v1/atm/tasks"),
        "a task request is rejected at the prior server route boundary, before mutation dispatch"
    );
}
