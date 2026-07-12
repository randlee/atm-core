#!/usr/bin/env bash
set -euo pipefail

atm log snapshot --limit 20
atm log filter --level warn --match command=send
