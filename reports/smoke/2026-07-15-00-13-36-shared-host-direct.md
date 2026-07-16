# Shared-host Direct Smoke

- status: `passed`
- binary SHA: `b610431fbba7728933d039cca4d1922668efa7cb`
- command: `python3 scripts/smoke/run_thorough_shared_host.py`

## Result

The direct shared-host smoke lane passed on macOS.

- daemon pids before: `[]`
- daemon pids during: `[64519]`
- note: `two workspaces with one shared ATM_HOME daemon/database/log root handled raw CLI send/read/ack traffic without cross-workspace leakage; invalid stdin failed locally without changing daemon PID set`
