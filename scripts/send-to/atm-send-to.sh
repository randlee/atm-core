#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "atm-send-to: provide at least one file" >&2
    exit 2
fi

send_to_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
files=("$@")
atm_bin=${ATM_BIN:-atm}
picker_override=${ATM_SEND_TO_PICKER:-}
# Test-only seam: overrides which native fallback picker runs once Wyvern is
# unavailable or incompatible, so degradation-harness coverage never depends
# on a host having zenity/fzf/osascript installed. Production launches never
# set this; the real OS-detected picker below remains the default.
native_picker_override=${ATM_SEND_TO_NATIVE_PICKER:-}
# AQ6 updates this exact-version constant during release preflight.
WYVERN_PIN="0.5.0"
wyvern_asset=${ATM_SEND_TO_WYVERN_ASSET:-"$send_to_dir/pick-member.html"}

input=$("$atm_bin" teams --json --members)
picker_output=
if [ -n "$picker_override" ]; then
    picker_output=$(printf '%s\n' "$input" | "$picker_override")
else
    if python3 "$send_to_dir/probe_wyvern.py" --pin "$WYVERN_PIN" --asset "$wyvern_asset" 2>/dev/null; then
        if picker_output=$(printf '%s\n' "$input" | "${ATM_SEND_TO_WYVERN_BIN:-wyvern}" --picker "$wyvern_asset"); then
            if ! printf '%s\n' "$picker_output" | python3 "$send_to_dir/picker.py" --validate >/dev/null; then
                echo "send-to: Wyvern returned an incompatible PickerOutput; using native picker" >&2
                picker_output=
            fi
        else
            echo "send-to: Wyvern picker failed; using native picker" >&2
            picker_output=
        fi
    else
        echo "send-to: Wyvern unavailable or incompatible; using native picker" >&2
    fi
    if [ -z "$picker_output" ]; then
        if [ -n "$native_picker_override" ]; then
            native_picker="$native_picker_override"
        else
            case "$(uname -s)" in
                Darwin) native_picker="$send_to_dir/picker-macos.sh" ;;
                *) native_picker="$send_to_dir/picker-linux.sh" ;;
            esac
        fi
        picker_output=$(printf '%s\n' "$input" | "$native_picker")
    fi
fi

# Validate before invoking atm send.  Cancellation, malformed output, and
# every optional-picker failure therefore have zero send side effects.
printf '%s\n' "$picker_output" | python3 "$send_to_dir/picker.py" --validate >/dev/null

# Construct repeated --attach options while preserving filenames verbatim.
send_args=("$atm_bin" send --from-json)
for file in "${files[@]}"; do
    send_args+=(--attach "$file")
done
printf '%s\n' "$picker_output" | "${send_args[@]}"
