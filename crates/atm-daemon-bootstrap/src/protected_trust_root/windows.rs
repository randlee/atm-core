//! Windows implementation of the protected trust-root reader.
//!
//! The configured location comes from the OS Known Folder API. Metadata and
//! contents are obtained through the same open handles; share modes prevent
//! replacement or a concurrent writer while those handles are held.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use atm_storage::AtmError;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, IsValidAcl, IsValidSid,
    OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
    FILE_SHARE_READ, FileAttributeTagInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
    READ_CONTROL,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};

use super::config_error;
use super::policy::{Ace, AceKind, Trustee, verify_dacl};

const TRUST_ROOT_NAME: &str = "tool-sender-trust-root.json";
const ATM_DIRECTORY: &str = "ATM";
const SECURITY_MAX_SID_SIZE: usize = 68;
pub(super) fn system_trust_root_path() -> Result<PathBuf, AtmError> {
    // SAFETY: SHGetKnownFolderPath writes a system-allocated, null-terminated
    // UTF-16 path on success. We release it with the matching CoTaskMemFree.
    let mut wide_path = null_mut();
    // SAFETY: output pointer is writable; the API allocates the resulting path.
    let status =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, null_mut(), &mut wide_path) };
    if status < 0 || wide_path.is_null() {
        return Err(AtmError::config(format!(
            "cannot locate Windows ProgramData folder (HRESULT {status:#x})"
        )));
    }
    let path = unsafe {
        let mut length = 0;
        while *wide_path.add(length) != 0 {
            length += 1;
        }
        let result = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
            wide_path, length,
        )));
        CoTaskMemFree(wide_path.cast());
        result
    };
    Ok(path.join(ATM_DIRECTORY).join(TRUST_ROOT_NAME))
}

pub(super) fn read_protected_file(path: &Path) -> Result<String, AtmError> {
    let parent = path
        .parent()
        .ok_or_else(|| config_error(path, "missing parent directory"))?;
    let parent_handle = open_handle(
        parent,
        FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    )
    .map_err(|error| config_error(path, format!("cannot open protected parent: {error}")))?;
    verify_file_type(&parent_handle, true)
        .map_err(|error| config_error(path, format!("unsafe parent directory: {error}")))?;
    verify_security(&parent_handle, true)
        .map_err(|error| config_error(path, format!("unsafe parent security: {error}")))?;

    let file_handle = open_handle(
        path,
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
        FILE_FLAG_OPEN_REPARSE_POINT,
    )
    .map_err(|error| config_error(path, format!("cannot open trust-root file: {error}")))?;
    verify_file_type(&file_handle, false)
        .map_err(|error| config_error(path, format!("unsafe trust-root file: {error}")))?;
    verify_security(&file_handle, false)
        .map_err(|error| config_error(path, format!("unsafe trust-root security: {error}")))?;

    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0_u8; 8192];
        let mut read = 0_u32;
        // SAFETY: the valid file handle is held for this call; `chunk` is a
        // writable buffer and the byte count points to a live u32.
        if unsafe {
            ReadFile(
                file_handle.0,
                chunk.as_mut_ptr(),
                chunk.len() as u32,
                &mut read,
                null_mut(),
            )
        } == 0
        {
            return Err(config_error(
                path,
                format!(
                    "cannot read trust-root file: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
    String::from_utf8(bytes)
        .map_err(|error| config_error(path, format!("trust-root file is not UTF-8: {error}")))
}

fn open_handle(path: &Path, access: u32, flags: u32) -> std::io::Result<Handle> {
    use std::os::windows::ffi::OsStrExt;

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Share read only: a pre-existing or later writer/deleter causes open to
    // fail, and the held handle prevents replacement during verification/read.
    // SAFETY: `wide_path` is null-terminated and all other arguments are valid.
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            access,
            FILE_SHARE_READ,
            null_mut(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Handle(handle))
}

fn verify_file_type(handle: &Handle, expect_directory: bool) -> Result<(), &'static str> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `info` is writable and has the documented size for this class.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err("cannot inspect file attributes");
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("reparse points are not permitted");
    }
    if (info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != expect_directory {
        return Err(if expect_directory {
            "parent is not a directory"
        } else {
            "trust root is not a regular file"
        });
    }
    Ok(())
}

fn verify_security(handle: &Handle, is_directory: bool) -> Result<(), &'static str> {
    let (descriptor, owner) = read_security_descriptor(handle)?;
    let _descriptor = descriptor;
    let (admins, system) = trusted_sids()?;
    let owner_is_trusted = owner_is_trusted(owner, &admins, &system);
    let (dacl_protected, dacl) = protected_dacl(_descriptor.0)?;
    let entries = read_dacl_entries(dacl, &admins, &system)?;
    verify_dacl(
        owner_is_trusted,
        dacl_protected,
        true,
        is_directory,
        &entries,
    )
}

fn read_security_descriptor(
    handle: &Handle,
) -> Result<(LocalSecurityDescriptor, *mut core::ffi::c_void), &'static str> {
    let mut owner = null_mut();
    let mut descriptor: *mut core::ffi::c_void = null_mut();
    // SAFETY: the output pointers are valid; the returned descriptor is freed
    // with LocalFree below. No SACL is requested.
    let status = unsafe {
        GetSecurityInfo(
            handle.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 || descriptor.is_null() {
        return Err("cannot read owner and DACL from opened handle");
    }
    Ok((LocalSecurityDescriptor(descriptor), owner))
}

fn owner_is_trusted(
    owner: *mut core::ffi::c_void,
    admins: &[u8; SECURITY_MAX_SID_SIZE],
    system: &[u8; SECURITY_MAX_SID_SIZE],
) -> bool {
    // SAFETY: `owner` was returned by GetSecurityInfo with a live descriptor.
    (unsafe { IsValidSid(owner) } != 0)
        && (unsafe { EqualSid(owner, admins.as_ptr().cast_mut().cast()) } != 0
            || unsafe { EqualSid(owner, system.as_ptr().cast_mut().cast()) } != 0)
}

fn protected_dacl(descriptor: *mut core::ffi::c_void) -> Result<(bool, *mut ACL), &'static str> {
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: descriptor is valid and the output pointers are writable.
    let dacl_protected =
        unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } != 0
            && control & SE_DACL_PROTECTED != 0;
    let mut dacl_present = 0;
    let mut dacl = null_mut::<ACL>();
    let mut dacl_defaulted = 0;
    // SAFETY: descriptor is valid and outputs are writable.
    let dacl_valid = unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    } != 0
        && dacl_present != 0
        && !dacl.is_null()
        && unsafe { IsValidAcl(dacl) } != 0;
    if !dacl_valid {
        return Err("DACL must be present, non-null, and valid");
    }
    Ok((dacl_protected, dacl))
}

fn read_dacl_entries(
    dacl: *mut ACL,
    admins: &[u8; SECURITY_MAX_SID_SIZE],
    system: &[u8; SECURITY_MAX_SID_SIZE],
) -> Result<Vec<Ace>, &'static str> {
    let mut entries = Vec::with_capacity(usize::from(unsafe { (*dacl).AceCount }));
    for index in 0..unsafe { (*dacl).AceCount } {
        let mut ace_ptr = null_mut();
        // SAFETY: ACL was validated and index is below AceCount.
        if unsafe { GetAce(dacl, u32::from(index), &mut ace_ptr) } == 0 || ace_ptr.is_null() {
            return Err("cannot inspect DACL entry");
        }
        // SAFETY: GetAce returns a pointer to a validated ACE in the ACL.
        let header = unsafe { &*ace_ptr.cast::<windows_sys::Win32::Security::ACE_HEADER>() };
        let (kind, trustee, mask) = if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 {
            (AceKind::Unsupported, Trustee::Other, 0)
        } else {
            if usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
                return Err("truncated DACL entry");
            }
            // SAFETY: ACE type and minimum size were checked above.
            let ace = unsafe { &*ace_ptr.cast::<ACCESS_ALLOWED_ACE>() };
            let sid = (&ace.SidStart as *const u32).cast_mut().cast();
            // SAFETY: the SID is inside a valid ACL ACE.
            if unsafe { IsValidSid(sid) } == 0 {
                return Err("invalid trustee SID in DACL");
            }
            let trusted = unsafe {
                EqualSid(sid, admins.as_ptr().cast_mut().cast()) != 0
                    || EqualSid(sid, system.as_ptr().cast_mut().cast()) != 0
            };
            (
                AceKind::Allow,
                if trusted {
                    Trustee::TrustedAdministrator
                } else {
                    Trustee::Other
                },
                ace.Mask,
            )
        };
        entries.push(Ace {
            kind,
            inherited: header.AceFlags != 0,
            trustee,
            mask,
        });
    }
    Ok(entries)
}

fn trusted_sids() -> Result<([u8; SECURITY_MAX_SID_SIZE], [u8; SECURITY_MAX_SID_SIZE]), &'static str>
{
    fn sid(kind: i32) -> Result<[u8; SECURITY_MAX_SID_SIZE], &'static str> {
        let mut storage = [0_u8; SECURITY_MAX_SID_SIZE];
        let mut size = SECURITY_MAX_SID_SIZE as u32;
        // SAFETY: storage has the documented maximum SID capacity and size is
        // initialized to that capacity.
        if unsafe { CreateWellKnownSid(kind, null_mut(), storage.as_mut_ptr().cast(), &mut size) }
            == 0
        {
            return Err("cannot construct trusted Windows SID");
        }
        Ok(storage)
    }
    Ok((sid(WinBuiltinAdministratorsSid)?, sid(WinLocalSystemSid)?))
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a valid handle returned by CreateFileW.
        unsafe { CloseHandle(self.0) };
    }
}

struct LocalSecurityDescriptor(*mut core::ffi::c_void);

impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: GetSecurityInfo allocated this descriptor with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}
