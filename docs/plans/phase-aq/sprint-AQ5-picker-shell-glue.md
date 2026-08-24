# Sprint AQ5 — Wyvern Picker and Shell Glue

Status: draft · Branch: `feature/aq-5-picker-shell-glue` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

The human-visible surface. Shell glue is paper-thin; all logic stays in the
`atm` stages (R13). Wyvern page work lands in the Wyvern repo; this sprint
carries the atm-core side plus the integration scripts and records the
Wyvern-side PR as a linked artifact.

## Deliverables

1. **Pipeline script** (versioned in-repo, e.g. `scripts/send-to/atm-send-to.sh`
   + `.ps1`): `atm teams --json | <picker> | atm send --attach "$@"
   --from-json`. Nonzero from any stage halts with no send (R5/R13).
2. **Fallback picker first**: macOS `osascript`/Shortcuts "Choose from list";
   Windows `Out-GridView`; Linux `zenity --list --checklist` (with a plain
   `fzf`/terminal fallback where no display server). Emits the PRD §4.2
   output JSON. This ships and remains the reference implementation.
3. **Wyvern `pick-member.html`** (Wyvern repo, linked PR): stdin JSON →
   member list grouped by team, dead/idle greyed (R4), multi-select, note
   field, output JSON on confirm, nonzero exit on cancel.
4. **Cold-start gate**: measured Wyvern launch-to-interactive on the Mac; if
   > 1 s, the fallback picker remains the default and the finding is
   recorded — Wyvern adoption is not forced by this sprint.
5. **Shell entries**: macOS Quick Action / Shortcut invoking the script;
   Windows `%APPDATA%\Microsoft\Windows\SendTo\*.lnk`; Linux (Ubuntu first)
   a Nautilus script in `~/.local/share/nautilus-scripts/` (drop-in, SendTo-
   class cost) plus a portable XDG `.desktop` "Open With" entry in
   `~/.local/share/applications/` for non-GNOME file managers (KDE service
   menus recorded as a follow-on). Install steps documented; no Share
   Extension / MSIX in this phase.

## Acceptance criteria

1. Script tests: cancel at picker → exit ≠ 0, zero sends (harness with a
   stub picker). Multi-file + multi-recipient happy path delivers.
2. Fixture test: fallback pickers' output JSON validates against PRD §4.2.
3. Cold-start measurement recorded with method + numbers in the PR.
4. Manual E2E evidence: Finder gesture on macOS, Explorer SendTo on Windows,
   and Nautilus script on Ubuntu each deliver to a live agent (transcript +
   screenshot committed).
5. `just test` unaffected crates remain green on all three CI lanes.

## Required validation

- Script harness under `.just/tests` (existing python-driven shell-script
  test convention, e.g. `test_release_gate.py`), per-lane where applicable
  (SendTo Windows-only, Quick Action macOS-only, Nautilus script
  ubuntu-only).
- Linked Wyvern PR reviewed; schema fixtures shared, not duplicated.

## Non-closure / out of scope

- Share Extension (signed bundle) and Win11 sparse-MSIX context menu.
- Any drafting/chat integration (PRD Phase 2).

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round.
- parallel_safe: AQ3, AQ4 (UI/scripts vs daemon internals).
