#[path = "registered_sender_issuer_fixture.rs"]
mod fixture;

use atm_storage::{RegisteredToolSender, RegisteredToolSenderIssuer};

fn main() {
    let issuer = fixture::InspectionIssuer::cron();
    let sender = RegisteredToolSender::from_issuer(&issuer);
    assert_eq!(sender.principal(), issuer.principal());
    assert_eq!(sender.address(), issuer.address());
}
