//! Windows file owner and DACL inspection. Unknown ACE types fail closed.
//!
//! Path-based checks inspect a snapshot. Use `VerifiedHandle` when an object
//! must remain stable between inspection and subsequent operations.

use std::ffi::c_void;
use std::fmt;
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
#[cfg(test)]
use windows_sys::Win32::Security::Authorization::GetNamedSecurityInfoW;
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetLengthSid,
    GetTokenInformation, IsValidAcl, IsValidSid, OWNER_SECURITY_INFORMATION, PSID, TOKEN_OWNER,
    TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ,
    FileAttributeTagInfo, GetFileInformationByHandleEx, OPEN_EXISTING, READ_CONTROL,
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
    /// The file owner, DACL, or file type does not meet the private-object policy.
    Insecure(&'static str),
}

impl fmt::Display for WindowsSecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System { operation, source } => write!(f, "{operation}: {source}"),
            Self::InvalidDescriptor(reason) => {
                write!(f, "invalid Windows security descriptor: {reason}")
            }
            Self::Insecure(reason) => write!(f, "insecure Windows object: {reason}"),
        }
    }
}
impl std::error::Error for WindowsSecurityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::System { source, .. } => Some(source),
            Self::InvalidDescriptor(_) | Self::Insecure(_) => None,
        }
    }
}

fn os_error(operation: &'static str) -> WindowsSecurityError {
    WindowsSecurityError::System {
        operation,
        source: std::io::Error::last_os_error(),
    }
}

struct Descriptor {
    _allocation: DescriptorAllocation,
    owner: PSID,
    dacl: *mut ACL,
}

struct DescriptorAllocation(*mut c_void);
impl Drop for DescriptorAllocation {
    fn drop(&mut self) {
        // SAFETY: Windows security-info APIs allocate the descriptor with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}

impl Descriptor {
    /// Build a descriptor owner and the component pointers returned with it by a Windows
    /// security-info API.
    ///
    /// # Safety
    /// `raw` must own a live security descriptor allocation, and `owner` and `dacl` must be
    /// the corresponding component pointers returned by that same API call.
    unsafe fn from_parts(
        raw: *mut c_void,
        owner: PSID,
        dacl: *mut ACL,
    ) -> Result<Self, WindowsSecurityError> {
        if raw.is_null() {
            return Err(WindowsSecurityError::InvalidDescriptor(
                "missing descriptor",
            ));
        }
        Ok(Self {
            _allocation: DescriptorAllocation(raw),
            owner,
            dacl,
        })
    }

    fn view(&self) -> Result<DescriptorView<'_>, WindowsSecurityError> {
        if self.owner.is_null() || self.dacl.is_null() {
            return Err(WindowsSecurityError::InvalidDescriptor(
                "missing owner or DACL",
            ));
        }
        // SAFETY: from_parts binds these pointers to the live allocation owned by self.
        let owner_sid = unsafe { copy_sid(self.owner)? };
        // SAFETY: from_parts guarantees dacl is a component pointer from this descriptor.
        let dacl = unsafe { &*self.dacl };
        Ok(DescriptorView { owner_sid, dacl })
    }
}

/// Component data borrowed from its owning Windows security descriptor.
struct DescriptorView<'a> {
    owner_sid: Vec<u8>,
    dacl: &'a ACL,
}
struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: Token owns the handle returned by OpenProcessToken.
        unsafe { CloseHandle(self.0) };
    }
}

fn token_information(
    token: HANDLE,
    information_class: windows_sys::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<usize>, WindowsSecurityError> {
    let mut needed = 0;
    // SAFETY: the null buffer requests the size for this token information class.
    unsafe { GetTokenInformation(token, information_class, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(os_error("GetTokenInformation size"));
    }
    // usize storage provides alignment for TOKEN_USER/TOKEN_OWNER and embedded pointers.
    let mut buffer = vec![0_usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: buffer is large and aligned enough for the requested token information class.
    if unsafe {
        GetTokenInformation(
            token,
            information_class,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(os_error("GetTokenInformation"));
    }
    Ok(buffer)
}

fn current_token_owner_sids() -> Result<(Vec<u8>, Vec<u8>), WindowsSecurityError> {
    let mut token = null_mut();
    // SAFETY: current process pseudo-handle is valid and token is a writable output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(os_error("OpenProcessToken"));
    }
    let token = Token(token);
    let user_buffer = token_information(token.0, TokenUser)?;
    let owner_buffer = token_information(token.0, TokenOwner)?;
    // SAFETY: successful responses begin with their corresponding token information structs;
    // both buffers stay alive through the copies below.
    let user_sid = unsafe { (*user_buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    // SAFETY: same guarantee as above for the TokenOwner response.
    let default_owner_sid = unsafe { (*owner_buffer.as_ptr().cast::<TOKEN_OWNER>()).Owner };
    // SAFETY: both pointers refer into their live, validated token-information buffers.
    let user_sid = unsafe { copy_sid(user_sid)? };
    // SAFETY: both pointers refer into their live, validated token-information buffers.
    let default_owner_sid = unsafe { copy_sid(default_owner_sid)? };
    Ok((user_sid, default_owner_sid))
}

/// Copy a Windows SID pointer into owned bytes.
///
/// # Safety
/// `sid` must point to a readable SID allocation for the duration of this call.
unsafe fn copy_sid(sid: PSID) -> Result<Vec<u8>, WindowsSecurityError> {
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

fn verify_owner_sid(
    owner_sid: &[u8],
    token_user_sid: &[u8],
    token_default_owner_sid: &[u8],
) -> Result<(), WindowsSecurityError> {
    if same_sid(owner_sid, token_user_sid) || same_sid(owner_sid, token_default_owner_sid) {
        Ok(())
    } else {
        Err(WindowsSecurityError::Insecure(
            "owner is neither the token user nor the token default owner",
        ))
    }
}

#[cfg(test)]
fn descriptor(path: &Path) -> Result<Descriptor, WindowsSecurityError> {
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
    // SAFETY: all three pointers were returned together by GetNamedSecurityInfoW above.
    unsafe { Descriptor::from_parts(raw, owner, dacl) }
}

/// Whether the path owner is the current process user.
///
/// # Errors
/// Returns a typed error if the owner or current-user SID cannot be read.
#[cfg(test)]
pub fn owned_by_current_user(path: &Path) -> Result<bool, WindowsSecurityError> {
    let descriptor = descriptor(path)?;
    let view = descriptor.view()?;
    let (token_user_sid, token_default_owner_sid) = current_token_owner_sids()?;
    Ok(verify_owner_sid(&view.owner_sid, &token_user_sid, &token_default_owner_sid).is_ok())
}

/// Whether every access-granting DACL entry is for the file owner.
///
/// A null, invalid or unsupported ACL is rejected; empty ACLs grant no access.
/// Owner Rights (S-1-3-4) is recognized as referring to the file owner.
/// # Errors
/// Returns a typed error if the descriptor cannot be inspected or interpreted.
#[cfg(test)]
pub fn dacl_grants_only_owner(path: &Path) -> Result<bool, WindowsSecurityError> {
    let descriptor = descriptor(path)?;
    let view = descriptor.view()?;
    dacl_only_owner(&view)
}

fn dacl_only_owner(descriptor: &DescriptorView<'_>) -> Result<bool, WindowsSecurityError> {
    let owner = &descriptor.owner_sid;
    let dacl = std::ptr::from_ref(descriptor.dacl).cast_mut();
    if unsafe { IsValidAcl(dacl) } == 0 {
        return Err(WindowsSecurityError::InvalidDescriptor(
            "null or invalid DACL",
        ));
    }
    // S-1-3-4 is the well-known OWNER RIGHTS SID.
    const OWNER_RIGHTS: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0];
    // SAFETY: IsValidAcl validated the live DACL and its header.
    for index in 0..descriptor.dacl.AceCount {
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
        // SAFETY: the SID range was bounded by the validated ACE header above.
        let sid = unsafe { copy_sid(sid_ptr.cast_mut().cast())? };
        if ace.Mask != 0 && !same_sid(&sid, owner) && !same_sid(&sid, OWNER_RIGHTS) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A verified Windows file or directory held open without write/delete sharing.
/// The same handle is used for security inspection, reads, and metadata.
pub struct VerifiedHandle {
    file: std::fs::File,
}

impl VerifiedHandle {
    /// Open without following a final reparse point, then verify owner and DACL.
    ///
    /// # Errors
    /// Rejects inaccessible, reparse-point, foreign-owned or shared objects.
    pub fn open(path: &Path) -> Result<Self, WindowsSecurityError> {
        let file = open_unshared(path)?;
        reject_reparse_point(&file)?;
        verify_handle_security(&file)?;
        Ok(Self { file })
    }

    /// Read the verified open file to completion without reopening its path.
    ///
    /// # Errors
    /// Returns an I/O error if reading the held handle fails.
    pub fn read_to_end(&mut self) -> Result<Vec<u8>, WindowsSecurityError> {
        let mut bytes = Vec::new();
        self.file
            .read_to_end(&mut bytes)
            .map_err(|source| WindowsSecurityError::System {
                operation: "read verified handle",
                source,
            })?;
        Ok(bytes)
    }

    /// Read metadata from the verified open handle.
    ///
    /// # Errors
    /// Returns an I/O error if querying the handle fails.
    pub fn metadata(&self) -> Result<std::fs::Metadata, WindowsSecurityError> {
        self.file
            .metadata()
            .map_err(|source| WindowsSecurityError::System {
                operation: "metadata on verified handle",
                source,
            })
    }
}

fn open_unshared(path: &Path) -> Result<std::fs::File, WindowsSecurityError> {
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(WindowsSecurityError::InvalidDescriptor("path contains NUL"));
    }
    wide.push(0);
    // SAFETY: wide is NUL-terminated and the other arguments follow CreateFileW's contract.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ,
            null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(os_error("CreateFileW"));
    }
    // SAFETY: CreateFileW returned a valid owned handle, transferred to File.
    let file = unsafe { std::fs::File::from_raw_handle(raw.cast()) };
    Ok(file)
}

fn reject_reparse_point(file: &std::fs::File) -> Result<(), WindowsSecurityError> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    // SAFETY: file owns a live handle and info is a correctly sized writable output.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(os_error("GetFileInformationByHandleEx"));
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(WindowsSecurityError::Insecure(
            "reparse points are not permitted",
        ));
    }
    Ok(())
}

fn verify_handle_security(file: &std::fs::File) -> Result<(), WindowsSecurityError> {
    let mut owner = null_mut();
    let mut dacl = null_mut();
    let mut raw_descriptor = null_mut();
    // SAFETY: file is live and all requested outputs are writable.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut raw_descriptor,
        )
    };
    if status != 0 {
        return Err(WindowsSecurityError::System {
            operation: "GetSecurityInfo",
            source: std::io::Error::from_raw_os_error(status as i32),
        });
    }
    if raw_descriptor.is_null() {
        return Err(WindowsSecurityError::InvalidDescriptor(
            "missing descriptor",
        ));
    }
    // SAFETY: all pointers were returned together by GetSecurityInfo above.
    let descriptor = unsafe { Descriptor::from_parts(raw_descriptor, owner, dacl)? };
    let view = descriptor.view()?;
    let (token_user_sid, token_default_owner_sid) = current_token_owner_sids()?;
    verify_owner_sid(&view.owner_sid, &token_user_sid, &token_default_owner_sid)?;
    if !dacl_only_owner(&view)? {
        return Err(WindowsSecurityError::Insecure(
            "DACL grants access to another principal",
        ));
    }
    Ok(())
}

/// Create one directory with the current user as owner and a protected,
/// inheritable owner-only DACL in the initial creation operation.
///
/// # Errors
/// Returns a typed error if the descriptor cannot be constructed or creation fails.
pub(crate) fn create_owner_only_directory(path: &Path) -> Result<(), WindowsSecurityError> {
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, InitializeSecurityDescriptor, SE_DACL_PROTECTED,
        SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, SetSecurityDescriptorControl,
        SetSecurityDescriptorDacl, SetSecurityDescriptorOwner,
    };
    use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(WindowsSecurityError::InvalidDescriptor("path contains NUL"));
    }
    wide.push(0);
    let sddl = "D:P(A;OICI;FA;;;OW)\0".encode_utf16().collect::<Vec<_>>();
    let mut raw = null_mut();
    // SAFETY: the SDDL is NUL-terminated and raw is a writable output.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut raw, null_mut())
    } == 0
    {
        return Err(os_error(
            "ConvertStringSecurityDescriptorToSecurityDescriptorW",
        ));
    }
    let parsed = DescriptorAllocation(raw);
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = null_mut();
    // SAFETY: parsed is live and all outputs are writable.
    if unsafe {
        GetSecurityDescriptorDacl(parsed.0.cast(), &mut present, &mut dacl, &mut defaulted)
    } == 0
        || present == 0
        || dacl.is_null()
    {
        return Err(WindowsSecurityError::InvalidDescriptor(
            "missing owner-only DACL",
        ));
    }
    let sid = current_token_owner_sids()?.0;
    let mut aligned_sid = vec![0_u32; sid.len().div_ceil(4)];
    // SAFETY: aligned_sid has enough writable storage and does not overlap sid.
    unsafe {
        std::ptr::copy_nonoverlapping(sid.as_ptr(), aligned_sid.as_mut_ptr().cast(), sid.len())
    };
    // SAFETY: the zeroed descriptor is initialized before use.
    let mut descriptor: SECURITY_DESCRIPTOR = unsafe { std::mem::zeroed() };
    let descriptor_ptr = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
    // SAFETY: descriptor is writable; SID and DACL remain live through CreateDirectoryW.
    if unsafe { InitializeSecurityDescriptor(descriptor_ptr, 1) } == 0
        || unsafe { SetSecurityDescriptorOwner(descriptor_ptr, aligned_sid.as_mut_ptr().cast(), 0) }
            == 0
        || unsafe { SetSecurityDescriptorDacl(descriptor_ptr, 1, dacl, 0) } == 0
        || unsafe {
            SetSecurityDescriptorControl(descriptor_ptr, SE_DACL_PROTECTED, SE_DACL_PROTECTED)
        } == 0
    {
        return Err(os_error("construct owner-only security descriptor"));
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor_ptr,
        bInheritHandle: 0,
    };
    // SAFETY: path and descriptor inputs remain valid through the call.
    if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
        return Err(os_error("CreateDirectoryW"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW};

    fn set_owner_to_current_user(path: &Path) {
        use windows_sys::Win32::Security::{
            InitializeSecurityDescriptor, OWNER_SECURITY_INFORMATION, SECURITY_DESCRIPTOR,
            SetSecurityDescriptorOwner,
        };
        let sid = current_token_owner_sids().unwrap().0;
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

    pub(crate) fn set_dacl(path: &Path, sddl: &str) {
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
        let descriptor = DescriptorAllocation(raw);
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
    fn verified_handle_blocks_replacement_and_reads_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record");
        let replacement = dir.path().join("replacement");
        std::fs::write(&path, b"original").unwrap();
        std::fs::write(&replacement, b"replacement").unwrap();
        set_owner_to_current_user(&path);
        set_dacl(&path, "D:P(A;;FA;;;OW)");
        let mut guard = VerifiedHandle::open(&path).expect("verify original record");
        assert!(guard.metadata().unwrap().is_file());
        assert!(
            std::fs::rename(&replacement, &path).is_err(),
            "open guard denies replacement"
        );
        assert_eq!(guard.read_to_end().unwrap(), b"original");
        drop(guard);
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
    }

    #[test]
    fn verified_handle_rejects_dacl_with_an_extra_principal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extra-principal");
        std::fs::write(&path, b"data").unwrap();
        set_owner_to_current_user(&path);
        set_dacl(&path, "D:P(A;;FA;;;OW)(A;;FW;;;WD)");

        assert!(matches!(
            VerifiedHandle::open(&path),
            Err(WindowsSecurityError::Insecure(
                "DACL grants access to another principal"
            ))
        ));
    }

    #[test]
    fn verified_handle_rejects_a_deny_ace_as_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deny-ace");
        std::fs::write(&path, b"data").unwrap();
        set_owner_to_current_user(&path);
        set_dacl(&path, "D:P(D;;FA;;;WD)(A;;FA;;;OW)");

        assert!(matches!(
            VerifiedHandle::open(&path),
            Err(WindowsSecurityError::InvalidDescriptor(
                "unsupported ACE type"
            ))
        ));
    }

    #[test]
    fn verified_handle_rejects_a_final_component_symlink() {
        use windows_sys::Win32::Storage::FileSystem::{
            CreateSymbolicLinkW, SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
        };

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        std::fs::write(&target, b"data").unwrap();
        let wide_link = link
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let wide_target = target
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are NUL-terminated and remain alive for the call.
        assert_ne!(
            unsafe {
                CreateSymbolicLinkW(
                    wide_link.as_ptr(),
                    wide_target.as_ptr(),
                    SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
                )
            },
            0,
            "Windows CI must support unprivileged file symlinks: {:?}",
            std::io::Error::last_os_error()
        );

        assert!(matches!(
            VerifiedHandle::open(&link),
            Err(WindowsSecurityError::Insecure(
                "reparse points are not permitted"
            ))
        ));
    }

    #[test]
    fn foreign_owner_sid_is_not_current_user() {
        // Pure comparison avoids depending on the CI machine's account SID.
        let owner = [1, 1, 0, 0, 0, 0, 0, 5, 1, 0, 0, 0];
        let other = [1, 1, 0, 0, 0, 0, 0, 5, 2, 0, 0, 0];
        assert!(same_sid(&owner, &owner));
        assert!(!same_sid(&owner, &other));
    }

    #[test]
    fn token_user_owner_sid_is_accepted() {
        let token_user = [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 100, 0, 0, 0, 200, 0, 0, 0, 44, 1, 0, 0, 233, 3,
            0, 0,
        ];
        let token_default_owner = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];

        assert!(verify_owner_sid(&token_user, &token_user, &token_default_owner).is_ok());
    }

    #[test]
    fn token_default_owner_sid_is_accepted() {
        let token_user = [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 100, 0, 0, 0, 200, 0, 0, 0, 44, 1, 0, 0, 233, 3,
            0, 0,
        ];
        let token_default_owner = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];

        assert!(verify_owner_sid(&token_default_owner, &token_user, &token_default_owner).is_ok());
    }

    #[test]
    fn foreign_owner_sid_is_rejected_when_token_is_elevated() {
        let token_user = [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 100, 0, 0, 0, 200, 0, 0, 0, 44, 1, 0, 0, 233, 3,
            0, 0,
        ];
        let token_default_owner = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];
        let foreign_owner = [
            1, 5, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 100, 0, 0, 0, 200, 0, 0, 0, 44, 1, 0, 0, 234, 3,
            0, 0,
        ];

        assert!(matches!(
            verify_owner_sid(&foreign_owner, &token_user, &token_default_owner),
            Err(WindowsSecurityError::Insecure(
                "owner is neither the token user nor the token default owner"
            ))
        ));
    }
}
