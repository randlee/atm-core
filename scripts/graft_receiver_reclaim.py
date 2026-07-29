#!/usr/bin/env python3
"""Run the ordered cross-process graft receiver crash/reclaim proof."""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]


def verify_cross_process_receiver_reclaim(root: pathlib.Path = ROOT) -> None:
    """Prove OS-lock release after an owner process exits without cleanup."""
    with tempfile.TemporaryDirectory(prefix="atm-graft-owner-") as fixture_root:
        environment = {**os.environ, "ATM_GRAFT_RECLAIM_CHILD_ROOT": fixture_root}
        for test_name in (
            "child_owner_exits_without_drop",
            "parent_reclaims_child_owner_lock",
        ):
            subprocess.run(
                [
                    "cargo",
                    "test",
                    "-p",
                    "agent-team-mail-core",
                    "--test",
                    "graft_receiver_ownership",
                    test_name,
                    "--",
                    "--ignored",
                    "--exact",
                ],
                check=True,
                cwd=root,
                env=environment,
            )


if __name__ == "__main__":
    verify_cross_process_receiver_reclaim()
