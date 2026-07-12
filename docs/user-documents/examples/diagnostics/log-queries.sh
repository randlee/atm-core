#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"
export ATM_IDENTITY="${ATM_IDENTITY:-arch-ctm}"

atm log snapshot --limit 20
atm log filter --level warn --match command=send
