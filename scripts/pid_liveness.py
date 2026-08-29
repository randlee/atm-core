"""Cross-platform "is this pid still running" check.

Shared by test/smoke tooling that needs to deterministically poll an
external process (a daemon under smoke test, a detached hook-script
child, ...) to completion instead of sleeping a fixed duration.

The one semantic every caller must get right: on POSIX, `os.kill(pid, 0)`
raising `PermissionError` proves the pid still names a live process (the
kernel found it and refused the signal for permission reasons) -- it is
not evidence the process is gone. Only `ProcessLookupError` means "gone".
"""

from __future__ import annotations

import os
import sys


def process_is_alive(pid: int) -> bool:
    """Return whether `pid` currently names a running process.

    POSIX: sends the null signal (`os.kill(pid, 0)`), which raises
    `ProcessLookupError` when the pid is gone. A `PermissionError` still
    proves the process exists (we just can't signal it) and counts as
    alive.

    Windows: there is no `os.kill(pid, 0)` liveness probe, so this opens
    the process with `PROCESS_QUERY_LIMITED_INFORMATION` and inspects its
    exit code via `GetExitCodeProcess`.
    """
    if sys.platform.startswith("win"):
        import ctypes

        query_limited_info = 0x1000
        still_active = 259
        handle = ctypes.windll.kernel32.OpenProcess(query_limited_info, False, pid)
        if not handle:
            return False
        try:
            exit_code = ctypes.c_ulong(0)
            if not ctypes.windll.kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code)):
                return False
            return exit_code.value == still_active
        finally:
            ctypes.windll.kernel32.CloseHandle(handle)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True
