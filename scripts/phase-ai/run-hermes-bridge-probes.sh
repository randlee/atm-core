#!/usr/bin/env bash
# Validate Hermes bridge registry entries; --active additionally probes launchd.
set -euo pipefail

usage() {
  echo "usage: $0 <profile-registry.tsv> [--active]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
registry=$1
active=${2:-}
[[ -f $registry ]] || { echo "registry not found: $registry" >&2; exit 2; }
[[ -z $active || $active == --active ]] || usage

if [[ $active == --active && $(uname -s) != Darwin ]]; then
  echo "--active launchd probes require macOS" >&2
  exit 2
fi

header=1
seen_profiles='|'
while IFS=$'\t' read -r profile team identity chat_id bridge_config log_path plist_path receiver_path extra; do
  if ((header)); then
    header=0
    [[ $profile == profile && $extra == '' ]] || { echo "invalid registry header" >&2; exit 2; }
    continue
  fi
  [[ -n $profile ]] || continue
  [[ -z $extra ]] || { echo "too many fields for profile $profile" >&2; exit 2; }
  [[ $profile =~ ^[A-Za-z0-9_-]+$ ]] || { echo "invalid profile: $profile" >&2; exit 2; }
  [[ $team =~ ^[A-Za-z0-9_-]+$ ]] || { echo "invalid ATM_TEAM for $profile" >&2; exit 2; }
  [[ $identity =~ ^[A-Za-z0-9_-]+$ ]] || { echo "invalid ATM_IDENTITY for $profile" >&2; exit 2; }
  [[ -z $chat_id || $chat_id =~ ^[A-Za-z0-9_-]+$ ]] || { echo "invalid ATM_CHAT_ID for $profile" >&2; exit 2; }
  [[ -n $bridge_config && -n $log_path && -n $plist_path && -n $receiver_path ]] || {
    echo "missing profile field for $profile" >&2; exit 2;
  }
  [[ $seen_profiles != *"|$profile|"* ]] || { echo "duplicate profile: $profile" >&2; exit 2; }
  seen_profiles+="$profile|"

  identity_rendered="$identity@$team"
  [[ -z $chat_id ]] || identity_rendered="$identity:$chat_id@$team"
  echo "validated $profile -> atm:$identity_rendered"

  if [[ $active == --active ]]; then
    label="ai.hermes.atm-graft-$profile"
    plutil -lint "$plist_path"
    launchctl print "gui/$UID/$label" >/dev/null
    [[ -S $receiver_path ]] || { echo "receiver unavailable for $profile: $receiver_path" >&2; exit 1; }
    launchctl kill SIGTERM "gui/$UID/$label"
    launchctl kickstart -k "gui/$UID/$label"
    launchctl print "gui/$UID/$label" >/dev/null
    echo "active supervision verified for $profile"
  fi
done < "$registry"
