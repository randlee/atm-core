//! Capability-authenticated loopback TCP transport adapter.
//!
//! This module owns only loopback listener admission and endpoint-record
//! lifecycle. It deliberately does not decode ATM request bodies or invoke
//! storage/hooks: after capability authentication it delegates every request
//! to the canonical AL.2 Axum router.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use atm_core::error::AtmError;
use atm_core::local_http::{LOCAL_CAPABILITY_HEADER, LocalCapability, LocalHttpEndpointRecord};
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use ulid::Ulid;

/// Loopback listener metadata owned by the TCP adapter.
///
/// The listener publishes only its actual loopback address and a fresh local
/// capability to `endpoint_record_path`. The daemon singleton owner is
/// composed elsewhere; its instance identifier is carried here solely to bind
/// that owner record to this endpoint publication.
#[derive(Debug, Clone)]
pub struct LoopbackTcpConfig {
    pub(crate) bind_address: SocketAddr,
    pub(crate) endpoint_record_path: PathBuf,
    pub(crate) daemon_instance_id: Ulid,
}

impl LoopbackTcpConfig {
    #[must_use]
    pub fn new(
        bind_address: SocketAddr,
        endpoint_record_path: PathBuf,
        daemon_instance_id: Ulid,
    ) -> Self {
        Self {
            bind_address,
            endpoint_record_path,
            daemon_instance_id,
        }
    }
}

pub(crate) fn validate_loopback_config(config: &LoopbackTcpConfig) -> Result<(), AtmError> {
    if !config.bind_address.ip().is_loopback() {
        return Err(preflight(
            "loopback_tcp.bind_address",
            "must be a loopback address",
        ));
    }
    if config.endpoint_record_path.as_os_str().is_empty() {
        return Err(preflight(
            "loopback_tcp.endpoint_record_path",
            "must not be empty",
        ));
    }
    Ok(())
}

fn preflight(field: &str, requirement: &str) -> AtmError {
    AtmError::config(format!("invalid runtime configuration field `{field}`"))
        .with_cause(requirement)
}

/// Adds only loopback peer and capability authentication to the canonical
/// router. The body, caller, and destination remain untouched; after this
/// middleware succeeds the existing local connector converts provenance to
/// `AuthenticatedIngress::Local` inside the one canonical handler.
pub(crate) fn authenticated_loopback_router(
    router: axum::Router,
    capability: LocalCapability,
) -> axum::Router {
    router.layer(middleware::from_fn_with_state(
        capability,
        authenticate_loopback_request,
    ))
}

async fn authenticate_loopback_request(
    State(capability): State<LocalCapability>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    if !peer.ip().is_loopback() {
        return crate::message_handler::error_response(AtmError::local_http_endpoint_non_loopback(
            "local HTTP request did not originate from a loopback peer",
        ));
    }

    let header_count = request
        .headers()
        .get_all(LOCAL_CAPABILITY_HEADER)
        .iter()
        .count();
    let presented = request.headers_mut().remove(LOCAL_CAPABILITY_HEADER);
    let is_valid = header_count == 1
        && presented
            .and_then(|value| value.to_str().ok().map(str::to_owned))
            .is_some_and(|value| capability.matches_header(&value));
    if !is_valid {
        return crate::message_handler::error_response(AtmError::local_http_capability_invalid(
            "local HTTP request must present the active loopback capability",
        ));
    }

    next.run(request).await
}

/// Owns precisely the endpoint-record generation published by this runtime.
/// Cleanup cannot remove a record written by a successor daemon instance.
#[derive(Debug)]
pub(crate) struct LoopbackEndpointRecordGuard {
    path: PathBuf,
    expected: LocalHttpEndpointRecord,
}

struct EndpointRecordParent {
    path: PathBuf,
    #[cfg(windows)]
    _verified: atm_core::windows_security::VerifiedHandle,
}

/// Removes the record only after the owned Axum task has stopped.
///
/// Endpoint-record I/O is synchronous filesystem work, so lifecycle code must
/// await it through Tokio's blocking pool rather than performing it in `Drop`
/// on a runtime worker.
pub(crate) async fn cleanup_loopback_endpoint_record(
    record: LoopbackEndpointRecordGuard,
) -> Result<(), AtmError> {
    tokio::task::spawn_blocking(move || record.cleanup_blocking())
        .await
        .map_err(|source| {
            AtmError::daemon_unavailable(
                "replacement loopback endpoint cleanup task ended unexpectedly",
            )
            .with_cause(source)
        })?
}

impl LoopbackEndpointRecordGuard {
    fn cleanup_blocking(self) -> Result<(), AtmError> {
        let contents = match std::fs::read(&self.path) {
            Ok(contents) => contents,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(AtmError::daemon_unavailable(
                    "failed to read local HTTP endpoint record during cleanup",
                )
                .with_cause(source));
            }
        };
        let Ok(current) = serde_json::from_slice::<LocalHttpEndpointRecord>(&contents) else {
            return Ok(());
        };
        if current == self.expected {
            std::fs::remove_file(&self.path).map_err(|source| {
                AtmError::daemon_unavailable("failed to remove local HTTP endpoint record")
                    .with_cause(source)
            })?;
        }
        Ok(())
    }
}

pub(crate) fn publish_loopback_endpoint_record(
    config: &LoopbackTcpConfig,
    endpoint: SocketAddr,
    capability: &LocalCapability,
) -> Result<LoopbackEndpointRecordGuard, AtmError> {
    let parent = prepare_endpoint_record_parent(config, endpoint)?;
    let record = active_endpoint_record(config.daemon_instance_id, endpoint, capability);
    let bytes = serialize_endpoint_record(&record)?;
    publish_private_endpoint_record(&config.endpoint_record_path, &parent.path, &bytes)?;
    Ok(LoopbackEndpointRecordGuard {
        path: config.endpoint_record_path.clone(),
        expected: record,
    })
}

fn prepare_endpoint_record_parent(
    config: &LoopbackTcpConfig,
    endpoint: SocketAddr,
) -> Result<EndpointRecordParent, AtmError> {
    if !endpoint.ip().is_loopback() {
        return Err(AtmError::local_http_endpoint_non_loopback(
            "local HTTP listener must bind only a loopback address",
        ));
    }
    let parent = config
        .endpoint_record_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            AtmError::local_http_runtime_directory_missing(
                "local HTTP endpoint record must have a runtime-directory parent",
            )
        })?;
    std::fs::create_dir_all(parent).map_err(|source| {
        AtmError::daemon_unavailable("failed to create local HTTP endpoint record directory")
            .with_cause(source)
    })?;
    #[cfg(windows)]
    let verified = validate_record_parent(parent)?;
    #[cfg(not(windows))]
    validate_record_parent(parent)?;
    Ok(EndpointRecordParent {
        path: parent.to_path_buf(),
        #[cfg(windows)]
        _verified: verified,
    })
}

fn serialize_endpoint_record(record: &LocalHttpEndpointRecord) -> Result<Vec<u8>, AtmError> {
    serde_json::to_vec(record).map_err(|source| {
        AtmError::new(
            atm_core::error::AtmErrorCode::SerializationFailed,
            "failed to serialize local HTTP endpoint record",
        )
        .with_cause(source)
    })
}

fn publish_private_endpoint_record(
    destination: &Path,
    parent: &Path,
    bytes: &[u8],
) -> Result<(), AtmError> {
    use std::fs;

    let (staging_path, file) =
        crate::private_staging::allocate(parent, "loopback", create_private_record_file).map_err(
            |source| {
                AtmError::daemon_unavailable("failed to create private local HTTP endpoint record")
                    .with_cause(source)
            },
        )?;
    if let Err(error) = write_and_publish_endpoint_record(file, &staging_path, destination, bytes) {
        let _ = fs::remove_file(&staging_path);
        return Err(error);
    }
    Ok(())
}

fn write_and_publish_endpoint_record(
    mut file: std::fs::File,
    staging_path: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), AtmError> {
    use std::io::Write;

    file.write_all(bytes).map_err(|source| {
        AtmError::daemon_unavailable("failed to write local HTTP endpoint record")
            .with_cause(source)
    })?;
    file.sync_all().map_err(|source| {
        AtmError::daemon_unavailable("failed to sync local HTTP endpoint record").with_cause(source)
    })?;
    restrict_record_to_owner(staging_path)?;
    std::fs::rename(staging_path, destination).map_err(|source| {
        AtmError::daemon_unavailable("failed to publish local HTTP endpoint record")
            .with_cause(source)
    })
}

fn active_endpoint_record(
    daemon_instance_id: Ulid,
    endpoint: SocketAddr,
    capability: &LocalCapability,
) -> LocalHttpEndpointRecord {
    let (ipv4_loopback, ipv6_loopback) = if endpoint.is_ipv4() {
        (Some(endpoint), None)
    } else {
        (None, Some(endpoint))
    };
    LocalHttpEndpointRecord::active(daemon_instance_id, ipv4_loopback, ipv6_loopback, capability)
}

#[cfg(unix)]
fn create_private_record_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_record_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn validate_record_parent(parent: &Path) -> Result<(), AtmError> {
    let metadata = std::fs::metadata(parent).map_err(|source| {
        AtmError::daemon_unavailable("failed to inspect local HTTP endpoint record directory")
            .with_cause(source)
    })?;
    if crate::unix_socket::parent_is_writable_by_others(&metadata) {
        return Err(AtmError::config(
            "local HTTP endpoint record directory must not be writable by others",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_record_parent(
    parent: &Path,
) -> Result<atm_core::windows_security::VerifiedHandle, AtmError> {
    match atm_core::windows_security::VerifiedHandle::open(parent) {
        Ok(verified) => Ok(verified),
        Err(atm_core::windows_security::WindowsSecurityError::Insecure(_)) => Err(
            AtmError::config("local HTTP endpoint record directory must not be writable by others"),
        ),
        Err(source) => Err(AtmError::daemon_unavailable(
            "failed to inspect local HTTP endpoint record directory",
        )
        .with_cause(source)),
    }
}

#[cfg(not(any(unix, windows)))]
fn validate_record_parent(_parent: &Path) -> Result<(), AtmError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_record_to_owner(path: &Path) -> Result<(), AtmError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|source| {
        AtmError::daemon_unavailable("failed to restrict local HTTP endpoint record to owner")
            .with_cause(source)
    })
}

#[cfg(not(unix))]
#[cfg(not(windows))]
fn restrict_record_to_owner(_path: &Path) -> Result<(), AtmError> {
    Ok(())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows owner-only ACL FFI is confined to loopback endpoint-record publication"
)]
fn restrict_record_to_owner(path: &Path) -> Result<(), AtmError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW,
    };

    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let descriptor_text = "D:P(A;;FA;;;OW)\0".encode_utf16().collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let mut descriptor_size = 0_u32;
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor_text.as_ptr(),
            1,
            &mut descriptor,
            &mut descriptor_size,
        )
    };
    if converted == 0 {
        return Err(AtmError::daemon_unavailable(
            "failed to create Windows owner-only local HTTP endpoint record ACL",
        ));
    }
    let applied = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe { LocalFree(descriptor) };
    if applied == 0 {
        return Err(AtmError::daemon_unavailable(
            "failed to restrict local HTTP endpoint record to its Windows owner",
        ));
    }
    Ok(())
}

/// Set a deterministic current-user owner and fixed DACL for Windows test records.
#[cfg(all(test, windows))]
pub(crate) fn set_test_record_security(path: &Path, sddl: &str) {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetTokenInformation, InitializeSecurityDescriptor,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_DESCRIPTOR,
        SetFileSecurityW, SetSecurityDescriptorOwner, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: the token is an owned handle returned by OpenProcessToken.
            unsafe { CloseHandle(self.0) };
        }
    }
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut raw_token = null_mut();
    // SAFETY: the process pseudo-handle is valid and output storage is writable.
    assert_ne!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) },
        0
    );
    let token = Token(raw_token);
    let mut needed = 0;
    // SAFETY: the null buffer requests the required token-information size.
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
    assert_ne!(needed, 0);
    let mut buffer = vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: buffer is aligned and has at least the requested number of bytes.
    assert_ne!(
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        },
        0
    );
    // SAFETY: a successful TokenUser response starts with TOKEN_USER.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    // SAFETY: initialization below fills this writable descriptor before use.
    let mut security: SECURITY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    let descriptor = (&mut security as *mut SECURITY_DESCRIPTOR).cast();
    // SAFETY: descriptor and token SID stay live through SetFileSecurityW.
    assert_ne!(unsafe { InitializeSecurityDescriptor(descriptor, 1) }, 0);
    assert_ne!(unsafe { SetSecurityDescriptorOwner(descriptor, sid, 0) }, 0);
    assert_ne!(
        unsafe { SetFileSecurityW(wide.as_ptr(), OWNER_SECURITY_INFORMATION, descriptor) },
        0
    );
    let text = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut dacl_descriptor = null_mut();
    // SAFETY: text is NUL-terminated and output pointer is writable.
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut dacl_descriptor,
                null_mut(),
            )
        },
        0
    );
    // SAFETY: the converted descriptor and path remain live through the call.
    let applied = unsafe {
        SetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            dacl_descriptor,
        )
    };
    // SAFETY: the conversion API returns a LocalAlloc allocation.
    unsafe { LocalFree(dacl_descriptor) };
    assert_ne!(applied, 0);
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    use atm_core::local_http::LocalCapability;
    use ulid::Ulid;

    use super::active_endpoint_record;

    #[test]
    fn endpoint_record_preserves_its_loopback_address_family() {
        let capability = LocalCapability::generate().expect("capability");
        let daemon_instance_id = Ulid::new();
        let ipv4 = active_endpoint_record(
            daemon_instance_id,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 43101)),
            &capability,
        );
        assert!(ipv4.ipv4_loopback.is_some());
        assert!(ipv4.ipv6_loopback.is_none());

        let ipv6 = active_endpoint_record(
            daemon_instance_id,
            SocketAddr::from((Ipv6Addr::LOCALHOST, 43101)),
            &capability,
        );
        assert!(ipv6.ipv4_loopback.is_none());
        assert!(ipv6.ipv6_loopback.is_some());
    }

    #[cfg(windows)]
    #[test]
    fn record_parent_rejects_everyone_writable_dacl() {
        let directory = tempfile::tempdir().expect("temporary parent directory");
        set_directory_dacl(directory.path(), "D:P(A;;FA;;;OW)(A;;FW;;;WD)");

        let error = super::validate_record_parent(directory.path())
            .err()
            .expect("Everyone-writable parent must be rejected");
        let expected = atm_core::error::AtmError::config(
            "local HTTP endpoint record directory must not be writable by others",
        );
        assert_eq!(error.message(), expected.message());
    }

    #[cfg(windows)]
    #[test]
    fn record_parent_accepts_owner_only_dacl() {
        let directory = tempfile::tempdir().expect("temporary parent directory");
        set_directory_dacl(directory.path(), "D:P(A;;FA;;;OW)");

        super::validate_record_parent(directory.path())
            .expect("owner-only parent directory must be accepted");
    }

    #[cfg(windows)]
    fn set_directory_dacl(path: &std::path::Path, sddl: &str) {
        use std::os::windows::ffi::OsStrExt;

        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            SetFileSecurityW,
        };

        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: SDDL is null terminated and descriptor is a writable output.
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            },
            0,
            "test DACL SDDL must convert"
        );

        // SAFETY: path and descriptor are valid for the duration of the call.
        let result = unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
        };
        // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW allocates via LocalAlloc.
        unsafe { LocalFree(descriptor) };
        assert_ne!(result, 0, "test directory DACL must be set from fixed SDDL");
    }
}
