# Post-mortem: HERMES-SKILL-TESTS, ATM 1.5.7, fixture: atm-hermes-testbed.local

Run of 2026-09-07, 20:58–21:13 UTC, on a freshly built image with no in-container patching
(`./teardown.sh`, `./run.sh --gateway`, then the seven sentences of `SMOKE-TEST-RUNBOOK.md` §5 in
order). Oversight agent: fenix. Tester: Claude Code inside the fixture (`tester@testbed`, driven by
ATM nudges through herdr). Hermes agent: `hermes@testbed`, fork base Hermes 0.21.0, driven with the
headless CLI as user hermes from the profile dir (the fixture gateway has no nudge adapter, #1307).
Inputs: ATM 1.5.7 tarball from the `prerelease/v1.5.7` prerelease-archive run, hermes_atm/atm_graft
1.5.7 wheels from the tag commit's ci.yml run, herdr v0.8.2, testbed PR randlee/atm-hermes-testbed#5
at b2294be (skills), image built 21:14 after the run's fixes.

Wall clock: bring-up 2 min (image build 3 min before it), first sentence 20:59:17, last report
21:12:45 — 13.5 minutes for seven sentences, seven reports. Every skill announced itself with a
START line 10–30 s after its sentence, every wait longer than 60 s produced a WAIT line, so no
minute of the run was spent wondering whether an agent was alive.

## Reports

| # | skill | agent | tools | result | elapsed | sentence → report |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | atm-setup-environment | tester | cli | PASS 6/6 | 20 s | 50 s |
| 2 | atm-setup-environment | hermes | cli | PASS 5/5 | 20 s | ~60 s |
| 3 | atm-smoke | tester | cli | PASS 8/8 (+ step 0) | 45 s | 89 s |
| 4 | atm-smoke | hermes | native | all 8 partner steps PASS; step 0 "BLOCKED" (finding S1); report in markdown, not the template (finding S2) | 35 s | 82 s |
| 5 | atm-hermes-ready | tester | cli | FAIL 3/4 — step 4 `no ack within 120s` (#1307, expected) | 125 s | 161 s |
| 6 | atm-nudge-roundtrip (tester) | tester | cli | FAIL 2/3 — step 2 `no ack within 120s` (finding S3); step 3 PASS on a non-observable (finding S4) | 125 s | 163 s |
| 7 | atm-nudge-roundtrip (responder) | hermes | native | PASS 4/4 — step 1 took 300 s (finding S5); `atm: client 0.0.0 daemon 0.0.0` (finding S6) | 310 s | 369 s |

Cross-host delivery worked for every line: 7 reports, 7 START lines, 3 WAIT lines and 2 probe
messages from the container reached `fenix@atm-dev.rand-m5` over the peer link; nothing was lost
or duplicated.

## FAIL lines and their causes

- **#5 step 4, `no ack within 120s`.** The hermes-atm receiver injects only into a Telegram adapter
  and the fixture gateway has none (#1307), so the ping sits in the Hermes agent's pending-ack list
  (it was still there after the run). This is the finding the skill exists to surface; it stays FAIL
  until #1307 lands.
- **#6 step 2, `no ack within 120s`.** The responder acked at 21:12:37, 6 minutes after the tester's
  send, because of finding S5. The ATM path itself is fine: in the 20:45 rehearsal of the same pair
  the ack arrived in 40 s (tester PASS 3/3, responder PASS 4/4 in 6 s).

## Interventions by the oversight agent

None during the clean run: no environment fix, no re-run, no nudge. Every defect that showed up
was a skill wording defect and was fixed between runs (below), never mid-run.

The clean run was the fifth iteration of the day on this fixture. The rehearsal iterations
(19:42–20:56) found and fixed, in the runbook, the harness and the skills, before the clean run:

| what broke | why | fix (testbed PR #5) | then |
| --- | --- | --- | --- |
| tester did not know the skills | image had no `.claude/skills` | Dockerfile copies skills to `/opt/testbed/.claude/skills`, symlinks for `.codex/skills`, `CLAUDE.md` → `AGENTS.md` | PASS |
| tester created team `atm-dev` in the fixture, sent report to `fenix@atm-dev` (no `.host`) | "requester"/"roster" ambiguous | skills: roster only from "expected roster:", report address copied verbatim with `.host` | PASS |
| fresh container: self round trip FAIL, random ports | localhost trust entry missing at daemon start (trust store is snapshotted) | `bringup.sh` adds the localhost trust before the daemon starts; skills forbid trust edits | PASS |
| `claude: No such file` | Claude Code not in the image | `install-claude-code.sh` at build | PASS |
| gateway not running on a fresh container | not s6-supervised | `bringup.sh` starts it and waits for "hook(s) loaded" | PASS |
| host → container sends impossible | daemon dials fixed port 43101 (#1309) | sentences enter via `docker exec -i … atm send`; reports leave over the link | works |
| hermes smoke quit at 182 s / filtered `from == "tester@testbed"` / used `jq` | wording | 600 s → later 300 s poll, `from` is the bare name, python3 not jq | PASS |
| reports with PENDING steps at 150 s, markdown instead of the template | rule lived only in the sibling `REPORT.md` | poll loop + "no PENDING, report after the deadline" in every SKILL.md | PASS |
| nothing told the oversight side an agent had started ("15 minutes of wondering") | no start signal | START line first, WAIT line every 60 s, deadlines cut to 120/300 s | PASS |
| tester's START line never arrived | it sent it to the nudge sender (`stub-alpha`) | `REPORT.md`: requester = the report address in the sentence, never the nudge sender | PASS |
| hermes ran `atm-setup-environment`'s steps inside `atm-smoke`, three START lines | smoke step 0 said "exactly step 4 of atm-setup-environment" | step 0 is self-contained; one START line | PASS |
| daemon restart race in bring-up ("failed to create decomposed_messages view") | old daemon still exiting | wait for exit, one retry | PASS |

## Findings about the skills (fixed after the run, commit b2294be, image rebuilt 21:14)

- **S1** Native `atm_send` cannot address yourself (ATM rejects self-addressed sends); the CLI
  `--host localhost` loop is the one form that works. Smoke step 0 is now CLI-only in the terminal.
- **S2** The Hermes agent (haiku) sometimes ignores a template that lives in a sibling file. The
  report shape is now inlined in every SKILL.md's Report section; `REPORT.md` keeps the rules.
- **S3** 120 s is too short for an ack that another *agent* must produce; 120 s stays only for the
  gateway pong in atm-hermes-ready. Roundtrip tester ack deadline is 300 s again.
- **S4** Roundtrip tester step 3 ("my message is no longer pending") read the tester's own
  `--pending-ack` list, which lists only messages the caller must ack. Step removed.
- **S5** The responder wrote a Python poll loop in Hermes' code-execution tool that shelled out to
  `atm list`; that subprocess has no `ATM_IDENTITY`, so every call failed silently and the loop ran to
  its 300 s deadline, after which native `atm_list()` found the row at once. Rule added to
  `REPORT.md` and the responder step: `atm` runs only in the terminal tool or natively, never inside
  code execution; poll = one call, `sleep 10`, repeat.
- **S6** The Hermes report carried `atm: client 0.0.0 daemon 0.0.0`. The line now must come from
  `atm doctor --json` in the terminal; guessing is forbidden.
- Minor: the Hermes START line once said `agent: hermes-agent`; the field is now pinned to
  `<you>@<team>`. Hermes' native sends show the sender as `hermes:1` (session suffix); watchers
  match on the bare name before `:`.

## Product observations

- **#1307** (pre-existing): no nudge adapter for a gateway without Telegram → atm-hermes-ready step 4
  FAIL by design; the Hermes side is driven headless. The headless recipe works and is in the runbook.
- **#1309** (filed today): `direct_peer_port()` is the fixed 43101 and
  `storage_and_nudge_router.rs` dials `<host>:43101`, ignoring the trust entry's `https_port`; on one
  machine 43101 is the host daemon, so host → container over the peer link is impossible. Sentences
  enter the fixture via `docker exec -i`; reports leave over the link (container → host works).
- `atm list --pending-ack` is inbox-only; there is no CLI view of "did my message get acked" other
  than the ack reply arriving (S4). Worth a line in the CLI reference, not a defect.
- Native sends from a headless Hermes session carry a `:1` suffix on the sender (`hermes:1@testbed`).
- Hermes 0.21.0 prints "Warning: Unknown toolsets: atm" on every headless start and a config-migrate
  WARNING for `/opt/data/config.yaml`; both harmless (tools work, gateway loads the hook). For loki.
- `testbed/harness/setup-mtls.sh` still prints a 16-hex-char fingerprint prefix; harmless, tidy later.

## Recommended changes

- **Skills**: all applied in b2294be (S1–S6). Next run is the first with the inlined report shape;
  watch report #4 and #7 for template compliance.
- **Fixture / testbed**: `run.sh` restarts the *host* daemon (`launchctl kickstart -k`) on every run
  because the trust store is snapshotted at daemon start — Rand should know this is part of the
  runbook. Nothing else: bring-up was clean twice in a row on a fresh image.
- **ATM**: #1309 (dial the trust entry's port) would remove the `docker exec -i` seam and let the
  runbook's `t()` become `atm send tester@testbed.<fixture>` from the host; #1307 would let the
  Hermes side run from a nudge like every other agent, making sentence 7 unnecessary. No new
  defects in 1.5.7 were found by these seven skills.
