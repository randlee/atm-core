#[path = "registered_sender_issuer_fixture.rs"]
mod fixture;

use atm_storage::ToolRuntimeAdmission;

fn main() {
    let inspection = fixture::InspectionIssuer::cron();
    let _admission = ToolRuntimeAdmission::from_issuer(&inspection);
}
