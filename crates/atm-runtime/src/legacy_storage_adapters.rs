use std::ops::Deref;
use std::sync::Arc;

use atm_core::boundary::{
    self, ConfigDoctor, MailStoreDoctor, MailStoreDoctorReport, RosterStoreDoctor,
    RosterStoreDoctorReport,
};
use atm_core::doctor::RuntimeDoctorPorts;
use atm_core::error::AtmError;
use atm_storage::contract::MessageStore as SharedMessageStore;

#[derive(Clone)]
pub(crate) struct StorageBackends<M>
where
    M: Deref<Target = dyn SharedMessageStore + Send + Sync>,
{
    pub(crate) messages: M,
}

impl<M> std::fmt::Debug for StorageBackends<M>
where
    M: Deref<Target = dyn SharedMessageStore + Send + Sync>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageBackends")
            .field("messages", &std::any::type_name::<M>())
            .finish()
    }
}

#[derive(Clone, Default)]
struct DefaultMailStoreDoctor;

#[derive(Clone, Default)]
struct DefaultRosterStoreDoctor;

pub(crate) fn runtime_doctor_ports(
    config_doctor: Arc<dyn ConfigDoctor + Send + Sync>,
) -> RuntimeDoctorPorts {
    RuntimeDoctorPorts {
        config_doctor,
        mail_store_doctor: Arc::new(DefaultMailStoreDoctor),
        roster_store_doctor: Arc::new(DefaultRosterStoreDoctor),
        herdr_breaker: Arc::new(atm_core::doctor::ClosedHerdrBreakerDoctor),
        herdr_endpoint: Arc::new(atm_core::doctor::ClosedHerdrEndpointDoctor),
    }
}
impl boundary::sealed::Sealed for DefaultMailStoreDoctor {}
impl boundary::sealed::Sealed for DefaultRosterStoreDoctor {}

impl MailStoreDoctor for DefaultMailStoreDoctor {
    fn inspect_mail_store(&self) -> Result<MailStoreDoctorReport, AtmError> {
        // This doctor has no backend-owned failure mode and reports no extra diagnostics.
        Ok(MailStoreDoctorReport::default())
    }
}

impl RosterStoreDoctor for DefaultRosterStoreDoctor {
    fn inspect_roster_store(&self) -> Result<RosterStoreDoctorReport, AtmError> {
        // This doctor has no backend-owned failure mode and reports no extra diagnostics.
        Ok(RosterStoreDoctorReport::default())
    }
}
