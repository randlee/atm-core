use atm_storage::AtmError;

/// Chosen same-host daemon transport.
///
/// Unix defaults to UDS. `ATM_LOCAL_TRANSPORT=tcp` is an explicit diagnostic
/// and parity-test mode; it never silently substitutes for an unavailable UDS
/// endpoint. Windows has only loopback TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDaemonTransport {
    #[cfg(unix)]
    UnixDomainSocket,
    TcpLoopback,
}

impl LocalDaemonTransport {
    pub(crate) fn resolve(value: Option<&str>) -> Result<Self, AtmError> {
        match value {
            None | Some("") => {
                #[cfg(unix)]
                {
                    Ok(Self::UnixDomainSocket)
                }
                #[cfg(not(unix))]
                {
                    Ok(Self::TcpLoopback)
                }
            }
            #[cfg(unix)]
            Some("uds") => Ok(Self::UnixDomainSocket),
            Some("tcp") => Ok(Self::TcpLoopback),
            #[cfg(unix)]
            Some(value) => Err(AtmError::validation(format!(
                "ATM_LOCAL_TRANSPORT must be `uds` or `tcp`; received `{value}`"
            ))),
            #[cfg(not(unix))]
            Some(value) => Err(AtmError::validation(format!(
                "ATM_LOCAL_TRANSPORT must be `tcp` on this platform; received `{value}`"
            ))),
        }
    }

    fn label(self) -> &'static str {
        match self {
            #[cfg(unix)]
            Self::UnixDomainSocket => "uds",
            Self::TcpLoopback => "tcp",
        }
    }
}

/// Resolves the selected same-host client transport without attempting a
/// connection. This makes the selected transport inspectable by CLI and graft
/// callers and keeps the explicit TCP diagnostic mode deterministic.
pub fn local_daemon_transport() -> Result<LocalDaemonTransport, AtmError> {
    let value = std::env::var("ATM_LOCAL_TRANSPORT").ok();
    let transport = LocalDaemonTransport::resolve(value.as_deref())?;
    tracing::debug!(
        transport = transport.label(),
        "selected daemon local transport"
    );
    Ok(transport)
}

#[cfg(test)]
mod tests {
    use super::LocalDaemonTransport;

    #[test]
    fn selection_is_explicit_and_platform_scoped() {
        #[cfg(unix)]
        {
            assert_eq!(
                LocalDaemonTransport::resolve(None).expect("Unix default"),
                LocalDaemonTransport::UnixDomainSocket
            );
            assert_eq!(
                LocalDaemonTransport::resolve(Some("tcp")).expect("TCP override"),
                LocalDaemonTransport::TcpLoopback
            );
            assert!(LocalDaemonTransport::resolve(Some("invalid")).is_err());
        }
        #[cfg(not(unix))]
        assert_eq!(
            LocalDaemonTransport::resolve(None).expect("platform default"),
            LocalDaemonTransport::TcpLoopback
        );
    }
}
