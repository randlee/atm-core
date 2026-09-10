use super::{HerdrQueueWakePump, HerdrQueueWakeStats};
use crate::runtime_health::IdleOpportunitySink;
use atm_core::boundary::IdleOpportunity;

impl atm_core::boundary::sealed::Sealed for HerdrQueueWakePump {}

impl IdleOpportunitySink for HerdrQueueWakePump {
    fn on_idle_opportunity(&self, opportunity: IdleOpportunity) {
        if self.runtime_health.is_draining() {
            return;
        }
        let pump = self.clone();
        self.runtime_health.record_idle_opportunity_dispatch();
        self.detached_hooks.observe_task(async move {
            let mut stats = HerdrQueueWakeStats::default();
            pump.run_idle_opportunity(opportunity, (pump.clock)(), &mut stats)
                .await;
        });
    }
}
