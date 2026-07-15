#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"
export ATM_IDENTITY="${ATM_IDENTITY:-arch-ctm}"

ATM_LOG=debug atm send quality-mgr@"$ATM_TEAM" "review smoke lane" --stderr-logs
