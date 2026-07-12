#!/usr/bin/env bash
set -euo pipefail

export ATM_TEAM="${ATM_TEAM:-atm-dev}"

atm doctor --team "$ATM_TEAM" --json
