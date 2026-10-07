---
name: prompt-rewrite-judge
description: Rewrite prompt text for conciseness and clarity with fresh subagents, then have Jev judge each rewrite for dropped, changed or unclear requirements. Use when tightening a prompt, template step or skill rule without losing any requirement.
---

# prompt-rewrite-judge

1. Write `original.txt` (the text, placeholders verbatim) and `key.json`: a JSON list, one string per requirement the text must keep.
2. Controls: in `cand/`, put `original.txt` and two copies of it, each with one requirement removed.
3. Rewrite: 3-4 fresh subagents in parallel, each given both files, told "keep every requirement in key.json, placeholders verbatim, the text's structure, no rationale" and to write `cand/rN.txt`; vary the emphasis (shortest, clearest, action order).
   When ATM is available, also use teammates on other models, frontier or one tier below (the user may name them): give each the same brief with absolute paths and `cand/<agent>.txt` as its output, via `atm task assign <agent> --task-id rewrite-<slug>-<agent> --stdin`; it closes the task when the file is written. Judge once every subagent has written and every teammate has closed.
4. Judge: `.claude/skills/prompt-rewrite-judge/scripts/judge.py --key key.json --original original.txt --out jev/ cand/*.txt` from the repository root (`--client` overrides `.claude/skills/atm-bd-orchestration/scripts/jev_client.py`).
5. Trust the run only if each control shows `keep=no` on its removed requirement and the original shows no drops and `changes_rule no`.
6. Pick: no drops and `changes_rule no`, then fewest words, then `ambiguous no`. Read the winner yourself against the key; report any disagreement with Jev.
7. None qualifies: give the flagged drops or changes to fresh rewriters; at most 2 more rounds.

Never answer for Jev; a failed request is rerun once, then recorded as failed with its verbatim error.
