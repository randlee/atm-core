use crate::error::AtmError;
use crate::service_runtime::LocalServiceRuntime;

type DefaultRuntimeFactory = fn() -> Result<LocalServiceRuntime, AtmError>;

/// Hidden bounded hook used only by `atm-daemon-bootstrap`.
pub fn install_retained_runtime_factory_for_daemon_bootstrap(factory: DefaultRuntimeFactory) {
    crate::service_runtime_store::install_default_runtime_factory(factory);
}

/// Hidden bounded hook used only by `atm-runtime-test-support`.
pub fn install_retained_runtime_factory_for_test_support(factory: DefaultRuntimeFactory) {
    crate::service_runtime_store::install_default_runtime_factory(factory);
}

/// Hidden bounded hook used only by daemon runtime composition.
pub fn install_retained_runtime_instance_for_daemon(runtime: LocalServiceRuntime) {
    crate::service_runtime_store::install_default_runtime_instance(runtime);
}
