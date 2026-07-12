#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"
export ATM_IDENTITY="${ATM_IDENTITY:-arch-ctm}"

atm send quality-mgr@"$ATM_TEAM" "review the current branch"
atm list quality-mgr@"$ATM_TEAM" --team "$ATM_TEAM" --as quality-mgr --json
atm peek quality-mgr@"$ATM_TEAM" --team "$ATM_TEAM" --as quality-mgr --json
atm read --team "$ATM_TEAM"
atm ack 01KRFK5QTF2R6NRS3Q0F8Z9K0S "received"
