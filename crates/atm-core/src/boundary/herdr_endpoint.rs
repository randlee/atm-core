use std::future::Future;
use std::pin::Pin;

use crate::api::RequestDeadline;
use crate::doctor::HerdrEndpointObservation;
use crate::team_admin::MembersList;

use super::sealed;

/// BOUNDARY-HerdrEndpointDoctor — typed endpoint diagnostics supplied by the
/// bootstrap composition root.
pub trait HerdrEndpointDoctor: sealed::Sealed + Send + Sync {
    fn observe<'a>(
        &'a self,
        roster: &'a MembersList,
        caller_deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Vec<HerdrEndpointObservation>> + Send + 'a>>;
}
