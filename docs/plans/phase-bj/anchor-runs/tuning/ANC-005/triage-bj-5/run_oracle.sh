#!/bin/zsh
# usage: run_oracle.sh <case-id> <repeat>
cd /Users/randlee/github/atm-core-worktrees/plan/phase-bj
T=docs/plans/phase-bj/anchor-runs/tuning/ANC-005/triage-bj-5
S=.claude/skills/plan-anchoring/scripts
id=$1; rep=$2
item=atm-bj-5; [[ $id == O10-bj-5 ]] || item="oracle-$id"
PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1 python3 $S/jev_judge.py judge --anchors $T/oracle/anchors.ANC-005.json --gate plan --anchor ANC-005 \
  --item-id $item --item-file $T/oracle/$id.txt --no-confirm --threshold 0.6 \
  --out $T/runs/$id.r$rep.json > $T/runs/$id.r$rep.stdout.json 2> $T/runs/$id.r$rep.stderr.txt
echo "$id r$rep exit=$?"
