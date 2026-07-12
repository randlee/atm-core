#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"
export ATM_IDENTITY="${ATM_IDENTITY:-arch-ctm}"

atm clear --team "$ATM_TEAM" --dry-run
atm clear --team "$ATM_TEAM" --older-than 7d
