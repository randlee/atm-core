use std::path::{Path, PathBuf};

pub use atm_core::protocol::{
    ATM_FRAME_FLAGS_V1, FramePayload, MessageKind, RequestId, read_frame, write_frame,
};
use atm_storage::AtmError;
use interprocess::local_socket::{GenericFilePath, Name, ToFsName};

pub fn daemon_local_ipc_name_from_path(endpoint_path: &Path) -> Result<Name<'static>, AtmError> {
    let normalized = platform_local_ipc_endpoint_path(endpoint_path.to_path_buf());
    normalized
        .into_os_string()
        .to_fs_name::<GenericFilePath>()
        .map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to map daemon local IPC endpoint {} to a supported platform-local IPC name",
                endpoint_path.display()
            ))
        })
}

#[cfg(windows)]
fn platform_local_ipc_endpoint_path(path: PathBuf) -> PathBuf {
    const WINDOWS_PIPE_PREFIX: &str = r"\\.\pipe\";

    let raw = path.to_string_lossy();
    if raw.starts_with(WINDOWS_PIPE_PREFIX) {
        return path;
    }

    let mut hash = 0xcbf29ce484222325u64;
    for byte in raw.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    let mut leaf = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "atm-daemon".to_string());
    leaf.retain(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if leaf.is_empty() {
        leaf = "atm-daemon".to_string();
    }

    PathBuf::from(format!(r"\\.\pipe\atm-{}-{hash:016x}", leaf))
}

#[cfg(not(windows))]
fn platform_local_ipc_endpoint_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
pub(crate) use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope, next_request_id,
    request_from_frame_payload, request_to_frame_payload, response_from_frame_payload,
    response_to_frame_payload,
};
