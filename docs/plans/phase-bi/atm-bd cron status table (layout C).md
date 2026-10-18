# atm-bd cron status table (layout C)

Oct 17, 2026 · @Rand Lee

## Purpose

`atm-bd cron` emits a narrow markdown status table, one row per sprint, so a phase can be read at a glance. Layout C keeps four columns: sprint, state, ci, pr. The state cell holds one icon per agent working the sprint, else one sprint-state icon: the agent working the sprint, or the sprint's own state when no agent is on it.

## Table shape

Four columns, one row per sprint. The icon columns are centered so the table stays narrow.

| Column | Shows | Values |
| --- | --- | --- |
| sprint | the sprint's bead id | `bi-1`, `bi-2`, and so on |
| state | one icon, chosen by the rule below | an agent, or the sprint state |
| ci | one icon | pass, fail, running, none |
| pr | an icon and the PR number, or `-` when there is no PR | open, draft, merged, closed |

The markdown the command emits has this form, with a glyph in each `<icon>` slot:

```markdown
| sprint | state | ci | pr |
|---|:-:|:-:|---|
| bi-1 | <icon> | <icon> | <icon> [#118](https://github.com/<owner>/<repo>/pull/118) |
```

## PR links

Each PR number in the pr column is a link, so `#123` opens the PR. The URL is built from repo information, not looked up per PR:

```text
https://github.com/<owner>/<repo>/pull/<number>
```

- **Base URL:** `gh repo view --json url -q .url` returns the repo URL as `gh` resolves it, which handles forks and default-repo settings. Fallback: parse the `origin` remote, turning either `https://github.com/<owner>/<repo>.git` or `git@github.com:<owner>/<repo>.git` into `https://github.com/<owner>/<repo>`.
- **Once per run:** compute the base URL once and reuse it for every row.
- **One PR exactly:** `gh pr view <number> --json url -q .url` returns its URL and confirms the PR exists.
- **Cell form:** `[#123](https://github.com/<owner>/<repo>/pull/123)`, after the icon.
- **Edge cases:** GitHub Enterprise uses its own host in place of `github.com`. An Azure DevOps repo uses `https://dev.azure.com/<org>/<project>/_git/<repo>/pullrequest/<number>`. When a fork opens PRs against an upstream repo, link the upstream repo, not `origin`.

## Layout C rule

The state cell shows one icon per active agent (Rand 2026-10-18: every active agent is drawn, the column widens by one cell per extra agent), else one icon picked in this order:

1. If an agent is working the sprint, show that agent's icon: a dev agent (fable, luna), severity, or QA.
2. QA uses the gavel, colored by its result: indigo while running, green on pass, red on fail.
3. If no agent is working the sprint, show the sprint's state icon (closed, blocked, ready, not\_ready, and the rest).

Because an agent wins over state, a blocked sprint that has an agent on it shows the agent. The lock appears only when nobody holds the sprint.

### ci fail rule

The ci cell names the failing platform when exactly one platform failed, and falls back to the bug otherwise:

- Only one of macOS, Windows or Linux failed: show that platform's icon in red.
- More than one platform failed: show the red bug.
- A general CI action failed, one that is not tied to a platform: show the red bug.

## Icon reference

Glyphs are Material Design Icons in the Nerd Font `md-` range. Codepoints were looked up by name in MDI 7.4.47.

&#91;image: Icon legend: PR states, agents, severity, QA, CI and sprint states\]

### PR states

| State | Icon name | Codepoint | Color |
| --- | --- | --- | --- |
| open | `source-pull` | U+F04C2 | `#2D81E0` |
| draft | `pencil-outline` | U+F0CB6 | `#FFA60D` |
| merged | `source-merge` | U+F062D | `#8957E5` |
| closed | `cancel` | U+F073A | `#F85149` |
| merge-check conflict | `source-branch` | U+F062C | `#F85149` |

### Agents, severity and QA

| Who | Icon name | Codepoint | Color |
| --- | --- | --- | --- |
| fable | `snowflake` | U+F0717 | `#FF9F80` |
| haiku | `close` | U+F0156 | `#FF6533` |
| sonnet | `multiplication` | U+F0382 | `#FF784D` |
| opus | `asterisk` | U+F06C4 | `#FF8C66` |
| astra | `creation` | U+F0674 | `#FFE066` |
| sol | `white-balance-sunny` | U+F05A8 | `#FFA60D` |
| terra | `earth` | U+F01E7 | `#2DE057` |
| luna | `moon-waning-crescent` | U+F0F65 | `#FFE066` |
| severity (Jev) | `cube` | U+F01A6 | `#E551BA` |
| QA running | `gavel` | U+F029B | `#4A5AF8` |
| QA pass | `gavel` | U+F029B | `#2EA043` |
| QA fail | `gavel` | U+F029B | `#F85149` |

### GitHub CI states

| State | Icon name | Codepoint | Color |
| --- | --- | --- | --- |
| ci pass | `source-branch` | U+F062C | `#2EA043` |
| ci fail, more than one platform or a general action | `bug` | U+F00E4 | `#F85149` |
| ci fail, macOS only | `apple` | U+F0035 | `#F85149` |
| ci fail, Windows only | `microsoft-windows` | U+F05B3 | `#F85149` |
| ci fail, Linux only | `linux` | U+F033D | `#F85149` |
| ci running | `weather-hurricane` | U+F0898 | `#58A6FF` |
| ci none | `dots-horizontal` | U+F01D8 | `#2D81E0` |
| unknown (sprint, ci or pr state not recognised, or GitHub data off, rate-limited or truncated) | `?` (plain text, not an icon) | U+003F | `#2D81E0` (Rand 2026-10-18, ANC-034) |

### Sprint states

| State | Icon name | Codepoint | Color |
| --- | --- | --- | --- |
| open | `circle-outline` | U+F0766 | `#2D81E0` |
| in\_progress | `timer-sand` | U+F051F | `#0077FF` |
| blocked | `lock` | U+F033E | `#FA8537` |
| deferred | `pause-circle` | U+F03E5 | `#FFB726` |
| closed | `check-circle` | U+F05E0 | `#2DE057` |
| pinned | `pin` | U+F0403 | `#42C3AB` |
| hooked | `hook` | U+F06E2 | `#2DE057` |
| accepted | `thumb-up` | U+F0513 | `#E551BA` |
| planned | `calendar-check` | U+F00EF | `#FFB726` |
| proposed | `lightbulb-on` | U+F06E8 | `#FFB726` |
| ready | `play-circle-outline` | U+F040D | `#0077FF` |
| not\_ready | `traffic-cone` | U+F137C | `#D97757` |

## Examples

Twenty sprints, split across two views that carry different data so the examples cover more states. bi-1 to bi-10 are on black. bi-11 to bi-20 are on white.

&#91;image: Layout C example: bi-1 to bi-10 on black, bi-11 to bi-20 on white\]

| Sprint | State cell shows | ci | Why |
| --- | --- | --- | --- |
| bi-1 | check-circle, green | pass | closed, no agent on it, merged PR #118 |
| bi-2 | gavel, indigo | running | QA is running |
| bi-3 | severity (Jev cube, pink) | bug | severity is working it, more than one platform failed |
| bi-4 | luna | none | luna is working it, draft PR #127 |
| bi-5 | fable | none | dev (fable) is working it, no PR yet |
| bi-6 | gavel, red | Windows | QA failed, and only Windows failed in CI |
| bi-7 | play-circle-outline, blue | none | ready, nobody on it |
| bi-8 | lock, orange | none | blocked, nobody on it |
| bi-9 | timer-sand, blue | running | in\_progress, nobody on it |
| bi-10 | hook, green | none | hooked, no PR yet |
| bi-11 | gavel, green | pass | QA passed |
| bi-12 | sol | running | sol is working it |
| bi-13 | terra | macOS | terra is working it, only macOS failed |
| bi-14 | astra | Linux | astra is working it, only Linux failed, PR has a merge conflict (#137) |
| bi-15 | opus | none | opus is working it, draft PR #138 |
| bi-16 | sonnet | bug | sonnet is working it, a general CI action failed |
| bi-17 | haiku | pass | haiku is working it |
| bi-18 | pause-circle, yellow | none | deferred, PR #141 closed |
| bi-19 | thumb-up, pink | pass | accepted, PR #142 merged |
| bi-20 | traffic-cone, orange | none | not\_ready, nobody on it |

## Rendering notes

- The markdown output carries glyphs only. Markdown has no color, so the colors apply only when the table is printed in a terminal through Rich or ANSI.
- Every icon needs a Nerd Font. Without one, the icons show as blank boxes.
- Icon cells hold no text label, which is what keeps the table narrow.
- Contrast is 4:1 or better on black for every color. On white several fall short: the yellows are 1.3 to 2.0:1 and terra green is 1.8:1.

## Open items

- [ ] Command flag for the table output: `--md-table`, or `--md --table`.
