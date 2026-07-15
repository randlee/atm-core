#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"
export ATM_IDENTITY="${ATM_IDENTITY:-arch-ctm}"

atm list quality-mgr@"$ATM_TEAM" --team "$ATM_TEAM" --as quality-mgr --json
atm peek quality-mgr@"$ATM_TEAM" --team "$ATM_TEAM" --as quality-mgr --json
atm read --team "$ATM_TEAM"
