# Sprint AQ5 — Send-To Surface and Phase Evidence

Status: implementation complete; host/QA evidence pending · Branch: `feature/aq-5-surface-evidence` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

The human-visible surface plus phase closure. Shell glue is paper-thin; all
logic stays in the `atm` stages (R13). Wyvern page work lands in the Wyvern
repo; this sprint carries the atm-core side and records the Wyvern PR as a
linked artifact.

## Deliverables

1. **Pipeline script** (`scripts/send-to/atm-send-to.sh` + `.ps1`):
   `atm teams --json --members | <picker> | atm send --attach "$@"
   --from-json`. Nonzero from any stage halts with no send (R5/R13);
   `atm send`'s stderr — including the canonical "File transfer to <host>
   not enabled…" error — is surfaced via the shell entry's notification
   mechanism, never swallowed. Scripts are adapters only: no picker may
   implement ATM addressing, hashing, or storage writes.
2. **Fallback picker first**: macOS `osascript` "Choose from list"; Windows
   `Out-GridView`; Linux `zenity --list --checklist` (plain `fzf` fallback
   headless). Emits exactly the PRD §4.2 `PickerOutput`; ships as the
   reference implementation.
3. **Wyvern `pick-member.html`** (Wyvern repo, linked PR): stdin JSON →
   members grouped by team, dead/idle greyed (R4), multi-select, note
   field, `PickerOutput` on confirm, nonzero on cancel. **Cold-start
   gate**: measured launch-to-interactive; if > 1 s the fallback stays
   default and the finding is recorded.
3a. **Wyvern dependency contract** — Wyvern is an **optional runtime
   dependency**, never a build-time or packaging dependency of atm-core,
   and never required for any test lane:
   - **Compatibility is schema-versioned, not just version-pinned.** The
     real contract is the `PickerInput`/`PickerOutput` JSON schema: the
     checked-in fixture carries a `schema_version`, the Wyvern page
     declares the version it implements, and the pipeline script treats a
     version it does not recognize exactly like a missing picker (falls
     back, records why).
   - **Pin-latest version policy (Rand, 2026-08-23)**: the minimum Wyvern
     version is pinned to the **most recent Wyvern release** at the time of
     each atm release, recorded as a single **exact-version constant**
     (e.g. `WYVERN_PIN="1.7.2"`, never a range or a caret/`>=` bound — the
     same exact-pin discipline the repo already applies to its Cargo-level
     sc-ecosystem dependency, `sc-composer = "=1.4.1"` at
     `crates/atm-template-sc-compose/Cargo.toml:18`, which AQ6 deliverable
     1 extends to `sc-observability`/`sc-observability-types`) in
     `scripts/send-to/atm-send-to.{sh,ps1}` and documented alongside the
     install steps. We always demand the most recent Wyvern — the pin is
     bumped to latest as part of every atm release preflight (AQ6), never
     left to accumulate a "supports everything after X" range that grows
     the integration surface; if the latest Wyvern release regresses the
     picker contract, AQ6's fix-forward escape hatch (a recorded, linked
     GH issue plus an explicit pinned-back constant, never a silent stale
     pin) is what keeps this policy from making an atm release hostage to
     an upstream break — the mechanics live in AQ6 deliverable 1, this
     sprint only consumes the resulting pin. The script probes `wyvern
     --version` and uses Wyvern only when the binary is on `PATH`, parses
     a version `>=` the pin, and the page asset resolves. Consequently the
     schema compatibility question is always "does the pinned (latest)
     Wyvern support the expected `schema_version`?" — verified by AQ6's
     preflight integration test, not assumed. **The probe runs under a
     short bounded deadline** (1–2 s, well inside the cold-start budget)
     with the child killed on expiry — `wyvern` is an arbitrary
     environment-provided executable resolved from `PATH`, the same trust
     tier as a transfer script, so it gets the same bounded-execution
     guarantee as ADR-055 (c). A hang is not a fallback. The floor is set
     to the Wyvern commit that lands `pick-member.html` (recorded with its
     PR).
   - **Degradation is silent-but-logged, never a failure**: absent,
     too-old, unparsable-version, probe-timeout, or missing-asset →
     native fallback picker with a one-line note on stderr. A Wyvern problem must never
     turn into a failed send or a blocked gesture.
   - **The linked Wyvern PR/commit and the schema-fixture revision are
     both recorded in the atm-core PR**; Wyvern source is not vendored.
4. **Shell entries**: macOS Quick Action/Shortcut; Windows
   `%APPDATA%\Microsoft\Windows\SendTo\*.lnk`; Ubuntu Nautilus script
   (`~/.local/share/nautilus-scripts/`) + portable XDG `.desktop` "Open
   With" entry (KDE service menus = follow-on). Install steps documented;
   no Share Extension / MSIX.
5. **Untrusted-attachment convention (R8)**: `CLAUDE.md` (+ authoritative
   agent-conventions doc) states files under `$ATM_TEMP/send-to/` named in
   Send-To message text are untrusted data — never instructions.
6. **Validation evidence** (`docs/plans/phase-aq/validation-evidence.md`,
   AN8/AN12 pattern): per requirement R1–R8 + R13–R15, the closing
   test/artifact with links (PASS/OPEN, 40-hex SHA). Live scenarios: US-2
   cross-host over a configured transfer script + the unconfigured-host
   canonical error; retained-tmux and Herdr `atm queue` wake paths observed
   live (AQ3 and AQ2.7 transcripts referenced); residue check with a
   short-TTL sweep config. Open-item
   register: Phase 2 (drafting/chat/attachments-metadata/note_source),
   team addressing, Share Extension/MSIX, the accepted queued-attachment
   TTL interaction.

## Acceptance criteria

1. Script harness: cancel → exit ≠ 0, zero sends; multi-file +
   multi-recipient happy path delivers (stub picker).
2. Fallback pickers' output validates against PRD §4.2 fixtures (shared
   with Wyvern, not duplicated), including the `schema_version` field.
2a. Wyvern dependency contract (deliverable 3a): harness cases prove
   picker selection falls back to the native picker — with the stderr note
   and a successful send — for each of: `wyvern` absent from `PATH`,
   version below the pin, unparsable `--version`, `--version` hanging
   past the probe deadline (child killed, treated as absent), unknown
   `schema_version`, missing page asset. No test lane requires Wyvern
   installed.
3. R8 text present in `CLAUDE.md`, names `$ATM_TEMP/send-to` (grep-check).
4. Manual E2E: Finder (macOS), Explorer SendTo (Windows), Nautilus
   (Ubuntu) each deliver to a live agent — transcript + screenshot
   committed. Cold-start numbers + method in the PR.
5. Every Must requirement maps to a passing gate; any gap is a Blocking
   finding, not a footnote. Evidence file reviewable by req-qa directly.
6. `just test` all three lanes green on the AQ5 head. (Phase closure — the
   `integrate/phase-aq` → `develop` merge PR — is AQ6's AC: AQ6 lands after
   this sprint and the phase must not close without it.)

## Paths to delete

None.

## Required validation

- Script harness under `.just/tests` (python convention, per-lane where
  applicable); linked Wyvern PR reviewed; full `just test` + integration
  suites on the final head.

## Non-closure / out of scope

- PRD Phase 2. Share Extension / Win11 MSIX. KDE service menus.

## Dependencies

- must_follow: AQ4 (consumes the CLI surface and staging behavior; queue
  evidence consumes AQ1–AQ3 and AQ2.6–AQ2.7) — merge-forward before every
  dev/fix round.
- parallel_safe: none remaining.

AQ5's automated implementation evidence is recorded in
[`validation-evidence.md`](validation-evidence.md). Physical Finder,
Explorer, and Nautilus runs remain explicitly OPEN until named host
operators attach real transcripts and screenshots; this sprint does not
claim those runs from a local macOS-only checkout.
