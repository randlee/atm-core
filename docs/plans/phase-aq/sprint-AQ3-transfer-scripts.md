# Sprint AQ3 — Cross-Host Transfer Scripts and Setup Doc

Status: draft · Branch: `feature/aq-3-transfer-scripts` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Ships the modifiable example scripts and the setup document behind the
canonical "not enabled" error. Setting up SSH or Tailscale at an org level
involves IT and cannot be planned around — so the product ships **examples a
human or agent adapts**, not managed infrastructure. No daemon code.

## Deliverables

1. **Example transfer scripts** under `scripts/send-to/transfer-examples/`:
   - `sftp.sh` (default recommendation): batch-mode `sftp`/`scp -r` to the
     destination's `$ATM_TEMP/send-to/<transfer-id>/`, assuming passwordless
     SSH (the fleet baseline); prints the landed dir per the AQ1 decision-(c)
     contract.
   - `tailscale.sh`: same contract over Tailscale SSH when available.
   - `rsync.sh`: rsync variant for large/repeated trees.
   - `sftp.ps1`: Windows counterpart.
   Each script is deliberately short, commented for modification, and
   exercises the exact invocation contract (`<script> <host> <transfer-id>
   <file>...` → landed dir on stdout, errors on stderr, nonzero on failure).
2. **`docs/cross-host-file-transfer.md`** (extends the AQ1 stub): copy an
   example to `~/.atm/transfer/<host>`, make it executable, adapt user/paths;
   troubleshooting section (key auth, unknown host, missing remote
   `$ATM_TEMP`); documents that the destination dir must be created by the
   script (`mkdir -p` over the same channel).
3. **Remote-`ATM_TEMP` note**: scripts resolve the destination's `ATM_TEMP`
   (documented approaches: fixed per-host value in the copied script, or
   `ssh <host> 'echo $ATM_TEMP'`); the chosen approach is the script owner's,
   examples show both.
4. **Script self-test harness**: a `.just/tests/` python test running each
   example against a loopback "remote" (localhost SSH where available, else
   a filesystem fake honoring the same contract) proving the contract:
   landed-dir stdout, stderr propagation, nonzero on induced failure.

## Acceptance criteria

1. Contract test: every shipped example, invoked per AQ1 decision (c),
   lands files and prints the landed dir; induced failure (bad host) exits
   nonzero with a human-actionable stderr.
2. `docs/cross-host-file-transfer.md` names the canonical error verbatim and
   walks one full setup (copy → chmod → adapt → verify with a dry run).
3. Live evidence: one real cross-host transfer (Mac → second host over the
   fleet's passwordless SSH) transcript committed, driven through
   `atm send --attach --from-json` end to end.
4. `just test` all three CI lanes (script harness skips lanes without a
   loopback SSH and says so — no silent skip).

## Paths to delete

None.

## Required validation

- `just test` + the script harness, ubuntu + macOS + Windows lanes (Windows
  runs `sftp.ps1` against the filesystem fake).
- Live cross-host demo transcript (AC 3) committed on the branch.

## Non-closure / out of scope

- Managed/automatic SSH or Tailscale enrollment — explicitly an environment
  concern (IT), documented not implemented.
- Any daemon involvement in transfer.

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round (consumes the
  CLI invocation seam and staging convention).
- parallel_safe: AQ4 (sweeper — disjoint), AQ5 (UI/scripts — the pipeline
  script work coordinates on the shared error-propagation behavior).
