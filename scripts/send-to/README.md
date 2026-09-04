# ATM Send-To shell entries

These files are deliberately thin adapters. They collect the roster through
`atm teams --json --members`, delegate selection to a picker, and invoke the
single final `atm send --from-json` command. They do not stage, hash, or copy
attachments.

## macOS Finder / Quick Action

Make `atm-send-to.sh`, `atm-send-to.command`, and the picker helpers
executable. In Automator, create a Quick Action receiving `files or folders`
in `Finder`, add a `Run Shell Script` action with `Pass input: as arguments`,
and invoke `atm-send-to.sh`. The `.command` wrapper can also be added to a
Shortcut that receives Finder files. The native picker is AppleScript's
`osascript Choose from list`; cancel exits nonzero and does not invoke `atm
send`. `atm-send-to.command` also raises a native `osascript display
notification` if the pipeline fails, since a Quick Action has no visible
terminal to show stderr in.

## Windows Explorer SendTo

Copy `atm-send-to.ps1` to
`%APPDATA%\\Microsoft\\Windows\\SendTo\\ATM Send-To.ps1`, then create a
shortcut in that same directory whose target is:

```text
pwsh.exe -NoProfile -File "%APPDATA%\Microsoft\Windows\SendTo\atm-send-to.ps1"
```

The target receives the selected files and uses `Out-GridView` for multi-
selection. PowerShell's normal stderr is left connected so transfer errors
and the canonical unconfigured-host message remain visible.

## Ubuntu Nautilus

```sh
mkdir -p "$HOME/.local/share/nautilus-scripts"
install -m 700 scripts/send-to/nautilus-atm-send-to.sh \
  "$HOME/.local/share/nautilus-scripts/ATM Send-To"
```

The selected files then appear under Nautilus' right-click **Scripts** menu.
`nautilus-atm-send-to.sh` also raises a `notify-send` desktop notification
(falling back to stderr alone when `notify-send` is not installed) if the
pipeline fails, since Nautilus Scripts run with no visible terminal.
For other file managers, copy `atm-send-to.desktop`, replace the placeholder
with an absolute path, and install it under
`~/.local/share/applications/atm-send-to.desktop`; refresh the desktop entry
cache if the file manager requires it. KDE service menus and Win11 MSIX are
follow-on work, not part of AQ5.

## Optional Wyvern

The exact pin is the `WYVERN_PIN` constant in each pipeline script (`0.6.0`,
the latest `randlee/wyvern` release observed on 2026-09-02). The optional
binary must be on `PATH`, satisfy the bounded `--version` probe, and find
the vendored `pick-member.html` asset (`scripts/send-to/pick-member.html`,
kept in sync with [`randlee/wyvern#140`](https://github.com/randlee/wyvern/pull/140)).
When all of that holds, the pipeline generates a small `wizard.json`
(`config` = the roster JSON) into `$ATM_TEMP` scratch and invokes
`wyvern <wizard.json> --ui-root <dir>` -- Wyvern has no `--picker` flag, and
its terminal stdout is a `WizardResult` envelope, not a bare `PickerOutput`;
see [`docs/plans/phase-aq/fixtures/wyvern-pick-member-contract.md`](../../docs/plans/phase-aq/fixtures/wyvern-pick-member-contract.md)
for the full shape. Absent, old, unparsable, hung, unknown-schema, or
missing-asset Wyvern always falls back to the native picker with a one-line
stderr note. No atm build or test lane requires Wyvern.
