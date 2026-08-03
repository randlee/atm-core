use std::io::Write;

use atm_core::error::AtmError;

/// Emit the supervisor-ready marker once, regardless of which local transport
/// owns the platform-specific daemon listener.
pub(crate) fn emit_ready_signal_if_requested() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_READY_STDOUT").is_none() {
        return Ok(());
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "ATM_DAEMON_READY").map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to emit daemon ready signal", source)
    })?;
    stdout.flush().map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to flush daemon ready signal", source)
    })?;
    Ok(())
}
