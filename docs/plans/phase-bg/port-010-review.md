# PORT-010 site review

Finding: ATM-PHASE-BG-REQ-002. The allowances below apply to Unix-specific
operations or helpers used exclusively by those operations. They are retained
pending Rand's per-site confirmation. Paths and line numbers refer to the
integration head at 6b7d374f1.

| Site | Decision | Reason |
| --- | --- | --- |
| `atm-core/src/atm_temp.rs:362` | Kept | Reads POSIX uid metadata. |
| `atm-core/src/atm_temp.rs:376` | Kept | Interprets Unix permission mode bits. |
| `atm-core/src/atm_temp.rs:384` | Kept | Calls POSIX `geteuid`. |
| `atm-core/src/atm_temp.rs:397` | Kept | Implements Unix uid-based scratch naming. |
| `atm-core/src/atm_temp_sweeper.rs:152` | Kept | Converts Unix ctime fields for expiry. |
| `atm-core/src/descriptor_limit.rs:92` | Kept | Reads errno for the POSIX descriptor-limit operation. |
| `atm-core/src/transfer_script.rs:129` | Kept | Defines Unix executable search directories. |
| `atm-core/src/transfer_script.rs:800` | Kept | Interprets POSIX owner-execute mode. |
| `atm-daemon-bootstrap/src/lib.rs:1013` | Kept | Configures Unix socket owner and mode. |
| `atm-daemon-bootstrap/src/protected_trust_root.rs:256` | Kept | Reads Unix ownership and mode metadata. |
| `atm-daemon-bootstrap/src/protected_trust_root.rs:283` | Kept | Validates Unix ownership and mode policy. |
| `atm-daemon-client/src/lib.rs:79` | Kept | Resolves the Unix socket endpoint path. |
| `atm-daemon-client/src/lib.rs:383` | Kept | Connects via Unix domain socket. |
| `atm-herdr/src/transport_socket.rs:353` | Kept | Connects via Tokio UnixStream. |
| `atm-http-runtime/src/client.rs:398` | Kept | Checks Unix runtime-directory ownership. |
| `atm-http-runtime/src/client_tail.rs:37` | Kept | Creates a Unix socket HTTP client. |
| `atm-http-runtime/src/client_tail.rs:107` | Kept | Configures reqwest Unix socket transport. |
| `atm-http-runtime/src/client_tail.rs:120` | Kept | Uses Unix socket metadata for generation identity. |
| `atm-http-runtime/src/http1_server.rs:260` | Kept | Serves HTTP on UnixListener. |
| `atm-http-runtime/src/lib.rs:338` | Kept | Accesses Unix socket owner uid. |
| `atm-http-runtime/src/lib.rs:363` | Kept | Accesses Unix socket permission mode. |
| `atm-http-runtime/src/runtime_listener.rs:52` | Kept | Binds and guards a Unix socket listener. |
| `atm-http-runtime/src/runtime_setup.rs:83` | Kept | Validates a Unix socket path. |
