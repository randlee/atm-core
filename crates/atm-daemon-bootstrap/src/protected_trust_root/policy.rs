//! Platform-independent policy for the protected Windows trust-root ACL.
//!
//! Keeping the decision separate from Win32 descriptor parsing lets the
//! security rules be tested deterministically on every build host.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Trustee {
    TrustedAdministrator,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AceKind {
    Allow,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Ace {
    pub kind: AceKind,
    pub inherited: bool,
    pub trustee: Trustee,
    pub mask: u32,
}

const FILE_READ_DATA: u32 = 0x0001;
const FILE_WRITE_DATA: u32 = 0x0002;
const FILE_APPEND_DATA: u32 = 0x0004;
const FILE_READ_EA: u32 = 0x0008;
const FILE_WRITE_EA: u32 = 0x0010;
const FILE_TRAVERSE: u32 = 0x0020;
const FILE_DELETE_CHILD: u32 = 0x0040;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
const DELETE: u32 = 0x0001_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const WRITE_OWNER: u32 = 0x0008_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const GENERIC_ALL: u32 = 0x1000_0000;
const READ_ONLY_RIGHTS: u32 =
    FILE_READ_DATA | FILE_READ_EA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;
const DANGEROUS_WRITE_RIGHTS: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | FILE_DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_WRITE
    | GENERIC_ALL;

pub(super) fn verify_dacl(
    owner_is_trusted: bool,
    protected: bool,
    present: bool,
    is_directory: bool,
    entries: &[Ace],
) -> Result<(), &'static str> {
    if !owner_is_trusted {
        return Err("owner must be SYSTEM or BUILTIN\\Administrators");
    }
    if !protected {
        return Err("DACL must be protected from inheritance");
    }
    if !present {
        return Err("DACL must be present and non-null");
    }
    for entry in entries {
        if entry.kind != AceKind::Allow || entry.inherited {
            return Err("only explicit, non-inheriting allow entries are permitted");
        }
        if entry.trustee == Trustee::TrustedAdministrator {
            continue;
        }
        if entry.mask & DANGEROUS_WRITE_RIGHTS != 0 {
            return Err(if is_directory {
                "untrusted trustee can modify or replace the protected parent"
            } else {
                "untrusted trustee can modify, replace, or change trust-root security"
            });
        }
        let allowed = READ_ONLY_RIGHTS | if is_directory { FILE_TRAVERSE } else { 0 };
        if entry.mask & !allowed != 0 {
            return Err("untrusted trustee has rights outside the read-only policy");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Ace, AceKind, Trustee, verify_dacl};

    fn other(mask: u32) -> Ace {
        Ace {
            kind: AceKind::Allow,
            inherited: false,
            trustee: Trustee::Other,
            mask,
        }
    }

    #[test]
    fn accepts_protected_acl_with_read_only_runner_and_trusted_admins() {
        let entries = [
            Ace {
                kind: AceKind::Allow,
                inherited: false,
                trustee: Trustee::TrustedAdministrator,
                mask: u32::MAX,
            },
            other(0x0012_0089), // FILE_GENERIC_READ
        ];
        assert!(verify_dacl(true, true, true, false, &entries).is_ok());
        assert!(verify_dacl(true, true, true, true, &[other(0x0012_00a9)]).is_ok()); // read + traverse
    }

    #[test]
    fn rejects_missing_or_unprotected_acl_and_untrusted_write_authority() {
        assert!(verify_dacl(true, false, true, false, &[]).is_err());
        assert!(verify_dacl(true, true, false, false, &[]).is_err());
        assert!(verify_dacl(false, true, true, false, &[]).is_err());
        assert!(verify_dacl(true, true, true, false, &[other(0x0002)]).is_err());
        assert!(verify_dacl(true, true, true, true, &[other(0x0040)]).is_err());
    }

    #[test]
    fn rejects_inherited_and_unknown_acl_entries() {
        let inherited = Ace {
            inherited: true,
            ..other(0x0012_0089)
        };
        let unknown = Ace {
            kind: AceKind::Unsupported,
            ..other(0x0012_0089)
        };
        assert!(verify_dacl(true, true, true, false, &[inherited]).is_err());
        assert!(verify_dacl(true, true, true, false, &[unknown]).is_err());
    }
}
