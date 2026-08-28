#!/bin/sh
set -eu
picker_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if command -v zenity >/dev/null 2>&1; then
    exec python3 "$picker_dir/picker.py" --backend zenity
fi
if command -v fzf >/dev/null 2>&1; then
    exec python3 "$picker_dir/picker.py" --backend fzf
fi
echo "send-to picker: install zenity or fzf, or set ATM_SEND_TO_SELECTION for a headless run" >&2
exit 1
