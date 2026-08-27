# AQ3 idle-drain and recovery-sweep evidence

Host: `local`
Commit: `c88fb3dc61b075b531479732453302de523bada6`
Status: **BLOCKED_AMBIENT_DAEMON**

Blocked by ambient `atm-daemon` pid(s) `[1816]`.
The runner fails closed because ATM's daemon owner and database scope is OS-account scoped.
Run on a dedicated account with no ambient daemon for positive-path evidence.
