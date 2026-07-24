# Windows VPN Peer Target Refresh

- Mac VPN target: `10.212.36.11` on `utun10`.
- Durable trust target changed from `192.168.128.82` to `10.212.36.11`; the
  certificate fingerprint was unchanged.
- Exactly one release daemon was controlled-restarted and remains healthy,
  ready, and bound to `10.10.100.98:43101`.
- `curl.exe` to `https://10.212.36.11:43101/` timed out at TCP after 5002ms;
  TLS and HTTP were not reached.
- ATM send to `arch-ctm@atm-dev.10.212.36.11` returned exit code 4 after
  3049ms due to the known local response deadline mismatch.

The RDP session proves Mac-to-Windows ingress from `10.212.36.11` to
`10.10.100.98`; it does not establish a Windows route back to the Mac VPN
address. No source or transport fallback was changed.
