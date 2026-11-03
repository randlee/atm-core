//! Windows file owner and DACL inspection. Unknown ACE types fail closed.
//!
//! These path-based checks inspect a security descriptor snapshot. Callers
//! that subsequently open a path must address replacement races themselves.

use std::ffi::c_void;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetLengthSid,
    GetTokenInformation, IsValidAcl, IsValidSid, OWNER_SECURITY_INFORMATION, PSID, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// An OS inspection failure or a security descriptor that cannot be safely interpreted.
#[derive(Debug)]
pub enum WindowsSecurityError {
    /// A Windows API failed, with the operation and its OS error.
    System {
        operation: &'static str,
        source: std::io::Error,
    },
    /// The returned security descriptor or ACL is invalid or unsupported.
    InvalidDescriptor(&'static str),
}

impl fmt::Display for WindowsSecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System { operation, source } => write!(f, "{operation}: {source}"),
            Self::InvalidDescriptor(reason) => {
                write!(f, "invalid Windows security descriptor: {reason}")
            }
        }
    }
}
impl std::error::Error for WindowsSecurityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::System { source, .. } => Some(source),
            Self::InvalidDescriptor(_) => None,
        }
    }
}

fn os_error(operation: &'static str) -> WindowsSecurityError {
    WindowsSecurityError::System {
        operation,
        source: std::io::Error::last_os_error(),
    }
}

struct Descriptor(*mut c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: GetNamedSecurityInfoW allocates the descriptor with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}
struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: Token owns the handle returned by OpenProcessToken.
        unsafe { CloseHandle(self.0) };
    }
}

fn current_user_sid() -> Result<Vec<u8>, WindowsSecurityError> {
    let mut token = null_mut();
    // SAFETY: current process pseudo-handle is valid and token is a writable output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(os_error("OpenProcessToken"));
    }
    let token = Token(token);
    let mut needed = 0;
    // SAFETY: the null buffer requests the required size.
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(os_error("GetTokenInformation size"));
    }
    // usize storage provides alignment for TOKEN_USER and its embedded pointer.
    let mut buffer = vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: buffer has at least `needed` writable bytes and suitable alignment.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(os_error("GetTokenInformation"));
    }
    // SAFETY: successful TokenUser response begins with TOKEN_USER; buffer remains live.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    copy_sid(sid)
}

fn copy_sid(sid: PSID) -> Result<Vec<u8>, WindowsSecurityError> {
    // SAFETY: SID pointers are supplied by Windows from a live descriptor or token.
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(WindowsSecurityError::InvalidDescriptor("invalid SID"));
    }
    // SAFETY: IsValidSid validated the SID; the owner allocation outlives this copy.
    let size = unsafe { GetLengthSid(sid) } as usize;
    Ok(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), size) }.to_vec())
}

fn same_sid(left: &[u8], right: &[u8]) -> bool {
    left == right
}

fn descriptor(path: &Path) -> Result<(Descriptor, PSID, *mut ACL), WindowsSecurityError> {
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(WindowsSecurityError::InvalidDescriptor("path contains NUL"));
    }
    wide.push(0);
    let mut owner = null_mut();
    let mut dacl = null_mut();
    let mut raw = null_mut();
    // SAFETY: path is NUL terminated and outputs are writable. Returned pointers
    // refer to the LocalAlloc descriptor, which the wrapper keeps alive.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut raw,
        )
    };
    if status != 0 {
        return Err(WindowsSecurityError::System {
            operation: "GetNamedSecurityInfoW",
            source: std::io::Error::from_raw_os_error(status as i32),
        });
    }
    if raw.is_null() {
        return Err(WindowsSecurityError::InvalidDescriptor(
            "missing descriptor",
        ));
    }
    Ok((Descriptor(raw), owner, dacl))
}

/// Whether the path owner is the current process user.
///
/// # Errors
/// Returns a typed error if the owner or current-user SID cannot be read.
pub fn owned_by_current_user(path: &Path) -> Result<bool, WindowsSecurityError> {
    let (_descriptor, owner, _) = descriptor(path)?;
    Ok(same_sid(&copy_sid(owner)?, &current_user_sid()?))
}

/// Whether every access-granting DACL entry is for the file owner.
///
/// A null, invalid or unsupported ACL is rejected; empty ACLs grant no access.
/// Owner Rights (S-1-3-4) is recognized as referring to the file owner.
/// # Errors
/// Returns a typed error if the descriptor cannot be inspected or interpreted.
pub fn dacl_grants_only_owner(path: &Path) -> Result<bool, WindowsSecurityError> {
    let (_descriptor, owner, dacl) = descriptor(path)?;
    let owner = copy_sid(owner)?;
    if dacl.is_null() || unsafe { IsValidAcl(dacl) } == 0 {
        return Err(WindowsSecurityError::InvalidDescriptor(
            "null or invalid DACL",
        ));
    }
    // S-1-3-4 is the well-known OWNER RIGHTS SID.
    const OWNER_RIGHTS: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0];
    // SAFETY: IsValidAcl validated the live DACL and its header.
    for index in 0..unsafe { (*dacl).AceCount } {
        let mut ptr = null_mut();
        // SAFETY: index is below AceCount and ptr is a writable output.
        if unsafe { GetAce(dacl, u32::from(index), &mut ptr) } == 0 || ptr.is_null() {
            return Err(os_error("GetAce"));
        }
        // SAFETY: GetAce returned an ACE within the valid DACL.
        let header = unsafe { &*ptr.cast::<ACE_HEADER>() };
        if header.AceType != 0 {
            return Err(WindowsSecurityError::InvalidDescriptor(
                "unsupported ACE type",
            ));
        }
        if usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
            return Err(WindowsSecurityError::InvalidDescriptor("truncated ACE"));
        }
        // SAFETY: the ACE type and minimum size are checked above.
        let ace = unsafe { &*ptr.cast::<ACCESS_ALLOWED_ACE>() };
        let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        let sid_capacity = usize::from(header.AceSize) - sid_offset;
        if sid_capacity < 8 {
            return Err(WindowsSecurityError::InvalidDescriptor("truncated SID"));
        }
        let sid_ptr = (&ace.SidStart as *const u32).cast::<u8>();
        // SAFETY: at least eight SID bytes lie within this ACE.
        let subauthorities = unsafe { *sid_ptr.add(1) } as usize;
        if 8 + subauthorities * 4 > sid_capacity {
            return Err(WindowsSecurityError::InvalidDescriptor("SID exceeds ACE"));
        }
        let sid = copy_sid(sid_ptr.cast_mut().cast())?;
        if ace.Mask != 0 && !same_sid(&sid, &owner) && !same_sid(&sid, OWNER_RIGHTS) {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW};

    fn set_owner_to_current_user(path: &Path) {
        use windows_sys::Win32::Security::{
            InitializeSecurityDescriptor, OWNER_SECURITY_INFORMATION, SECURITY_DESCRIPTOR,
            SetSecurityDescriptorOwner,
        };
        let sid = current_user_sid().unwrap();
        // Provide DWORD-aligned SID storage to the Windows APIs.
        let mut aligned_sid = vec![0_u32; sid.len().div_ceil(4)];
        // SAFETY: storage has at least sid.len() writable bytes and does not overlap sid.
        unsafe {
            std::ptr::copy_nonoverlapping(sid.as_ptr(), aligned_sid.as_mut_ptr().cast(), sid.len());
        }
        // SAFETY: zeroed storage is initialized immediately by the Windows API.
        let mut security: SECURITY_DESCRIPTOR = unsafe { std::mem::zeroed() };
        // SAFETY: security is writable and the aligned SID stays alive through SetFileSecurityW.
        assert_ne!(
            unsafe {
                InitializeSecurityDescriptor((&mut security as *mut SECURITY_DESCRIPTOR).cast(), 1)
            },
            0
        );
        assert_ne!(
            unsafe {
                SetSecurityDescriptorOwner(
                    (&mut security as *mut SECURITY_DESCRIPTOR).cast(),
                    aligned_sid.as_mut_ptr().cast(),
                    0,
                )
            },
            0
        );
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        assert_ne!(
            unsafe {
                SetFileSecurityW(
                    wide.as_ptr(),
                    OWNER_SECURITY_INFORMATION,
                    (&mut security as *mut SECURITY_DESCRIPTOR).cast(),
                )
            },
            0
        );
    }

    fn set_dacl(path: &Path, sddl: &str) {
        let text = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let wide_path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut raw = null_mut();
        // SAFETY: text is NUL-terminated and raw is a writable output.
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    1,
                    &mut raw,
                    null_mut(),
                )
            },
            0
        );
        let descriptor = Descriptor(raw);
        // SAFETY: path and descriptor remain valid throughout the call.
        assert_ne!(
            unsafe {
                SetFileSecurityW(
                    wide_path.as_ptr(),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    descriptor.0,
                )
            },
            0
        );
    }

    #[test]
    fn owner_only_sddl_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("owner-only");
        std::fs::write(&path, b"data").unwrap();
        set_owner_to_current_user(&path);
        set_dacl(&path, "D:P(A;;FA;;;OW)");
        assert!(owned_by_current_user(&path).unwrap());
        assert!(dacl_grants_only_owner(&path).unwrap());
    }

    #[test]
    fn everyone_write_sddl_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("everyone-write");
        std::fs::write(&path, b"data").unwrap();
        set_owner_to_current_user(&path);
        set_dacl(&path, "D:P(A;;FA;;;OW)(A;;FW;;;WD)");
        assert!(!dacl_grants_only_owner(&path).unwrap());
    }

    #[test]
    fn foreign_owner_sid_is_not_current_user() {
        // Pure comparison avoids depending on the CI machine's account SID.
        let owner = [1, 1, 0, 0, 0, 0, 0, 5, 1, 0, 0, 0];
        let other = [1, 1, 0, 0, 0, 0, 0, 5, 2, 0, 0, 0];
        assert!(same_sid(&owner, &owner));
        assert!(!same_sid(&owner, &other));
    }
}
