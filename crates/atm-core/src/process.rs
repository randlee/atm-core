#[cfg(unix)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    let pid: libc::pid_t = match pid.try_into() {
        Ok(pid) => pid,
        Err(_) => return false,
    };
    // SAFETY: libc::kill with signal 0 performs an existence/permission check
    // only and does not deliver a signal.
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }

    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess is called read-only for process liveness inspection.
    let process_id: u32 = pid;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return false;
    }

    let mut exit_code = 0u32;
    // SAFETY: handle was returned by OpenProcess and remains valid until CloseHandle below.
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    // SAFETY: handle was opened successfully above and must be closed once.
    unsafe { CloseHandle(handle) };
    ok != 0 && exit_code == STILL_ACTIVE as u32
}

#[cfg(test)]
mod tests {
    use super::process_is_alive;

    #[test]
    fn process_is_alive_reports_current_process() {
        assert!(process_is_alive(std::process::id()));
    }
}
