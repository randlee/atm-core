#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"
export ATM_IDENTITY="${ATM_IDENTITY:-arch-ctm}"

# Inspection-only paths may target another mailbox explicitly.
atm list quality-mgr@"$ATM_TEAM" --team "$ATM_TEAM" --as quality-mgr
atm peek quality-mgr@"$ATM_TEAM" --team "$ATM_TEAM" --as quality-mgr

# Mutating paths act as the resolved caller.
atm send quality-mgr@"$ATM_TEAM" "review failing smoke lane"
atm read --team "$ATM_TEAM"
atm ack 01KRFK5QTF2R6NRS3Q0F8Z9K0S "working on it"
