# Shared-host Direct Smoke

- status: `passed`
- binary SHA: `fcf4cd25edbc89a86c575f91c47241161fd7e4a9`
- command: `python3 scripts/smoke/run_thorough_shared_host.py`

## Result

The direct shared-host smoke lane passed on macOS.

- daemon pids before: `[]`
- daemon pids during: `[42366]`
- note: `two workspaces with one shared ATM_HOME daemon/database/log root handled raw CLI send/read/ack traffic without cross-workspace leakage; invalid stdin failed locally without changing daemon PID set`
