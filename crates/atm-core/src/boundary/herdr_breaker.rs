use crate::doctor::HerdrBreakerDoctorReport;

use super::sealed;

/// BOUNDARY-HerdrBreakerDoctor — typed breaker diagnostics supplied by the
/// bootstrap composition root.
pub trait HerdrBreakerDoctor: sealed::Sealed + Send + Sync {
    fn report(&self) -> HerdrBreakerDoctorReport;
}
