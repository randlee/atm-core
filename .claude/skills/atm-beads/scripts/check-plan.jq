# Gate for a rendered bead plan: run on the import JSONL before `bd import`.
#   jq -rs -f scripts/check-plan.jq plan.jsonl                                  # whole phase (root in the file)
#   jq -rs --arg root <prefix>-phase-d --arg phase d -f scripts/check-plan.jq plan.jsonl  # sprints added to an existing root
# --arg integration_branch <pattern> is the manifest's `integration_branch`
# (`{phase}` = the phase id; default integrate/phase-{phase}); validate-plan passes it.
# A dev bead may also be blocked by a plan-review bead <root>-plan-qa[-<n>].
# Prints problems and exits 5 if any.
def need($c; $msg): if $c then empty else $msg end;
def blank: . == null or . == "" or . == [];
def has_label($l): (.labels // []) | index($l) != null;
def deps($t): [(.dependencies // [])[] | select(.type == $t) | .depends_on_id];
# transitive closure of a set of ids over a {id: [prerequisite ids]} graph
def closure($g): until((. as $s | [$s[] | ($g[.] // [])[]] + $s | unique) == .; . as $s | [$s[] | ($g[.] // [])[]] + $s | unique);

. as $all
| [ $all[] | select(.issue_type == "epic" or .issue_type == "feature") ] as $roots
| [ $all[] | select(has_label("stage:dev")) ] as $devs
| [ $all[] | select(has_label("stage:dev-sanity")) ] as $qcs
| ($roots[0] // {}) as $root
| ($root.id // $ARGS.named.root // "") as $rid
| ($root.metadata.phase // $ARGS.named.phase // "") as $p
| ($root.metadata.integration_branch // (($ARGS.named.integration_branch // "integrate/phase-{phase}") | sub("\\{phase\\}"; $p))) as $trunk
| ([$qcs[] | {key: .id, value: (.metadata.dev_bead // "")}] | from_entries) as $qcdev
| ([$devs[] | {key: .id, value: [deps("blocks")[] | select(endswith("-sanity")) | ($qcdev[.] // rtrimstr("-sanity"))]}] | from_entries) as $g
| [
    need($roots | length <= 1; "plan has \($roots | length) roots; at most one"),
    need($devs | length > 0; "plan has no stage:dev beads"),
    need($rid != ""; "no root: include the phase root or pass --arg root <id> --arg phase <p>"),
    need($p | test("^[a-z]+$"); "phase id \($p | tojson) is not lower-case letters (a..z, aa, ab, ...)"),
    ( if ($roots | length) == 1 then need($root.title // "" | startswith("phase-\($p): "); "root title must start with phase-\($p): ") else empty end ),
    ( $all[] | select(.issue_type != "epic" and .issue_type != "feature") | . as $b
      | need(has_label("stage:dev") or has_label("stage:dev-sanity"); "\($b.id): neither stage:dev nor stage:dev-sanity"),
        need(has_label("phase-\($p)"); "\($b.id): missing label phase-\($p)"),
        need(deps("parent-child") == [$rid]; "\($b.id): parent is not the plan root \($rid)") ),
    ( $devs[] as $s | $s.metadata as $m | "\($s.id)" as $id
      | need($m.phase == $p; "\($id): phase \($m.phase | tojson) is not the plan phase \($p | tojson)"),
        need($m.sprint | test("^\($p)-[1-9][0-9]*$"); "\($id): sprint \($m.sprint | tojson) is not \($p)-<n>"),
        need($m.stack == "phase-\($p)"; "\($id): stack \($m.stack | tojson) is not phase-\($p) (the phase is one stack)"),
        need($m.branch | startswith("sprint/\($m.sprint)-"); "\($id): branch \($m.branch | tojson) is not sprint/\($m.sprint)-<slug>"),
        ( ["assignee", "description", "design", "acceptance_criteria"][] as $f
          | need($s[$f] | blank | not; "\($id): empty \($f)") ),
        ( ["stack", "layer", "branch", "pr_target", "worktree", "relation", "closure_type", "target_boundary", "owned_paths"][] as $k
          | need($m[$k] | blank | not; "\($id): empty metadata.\($k)") ),
        ( [["requirements", "^[A-Z][A-Z0-9]*(-[A-Z0-9]+)*-[0-9]+$", "REQ ids such as REQ-ATM-CMD-001"], ["adrs", "^ADR-[0-9]+$", "ADR-<n> ids"]][] as [$k, $re, $what]
          | $m[$k] as $v
          | if ($v | type) != "array" or ($v | length) == 0 then "\($id): metadata.\($k) is empty: list the governing \($what), or [\"NONE\"]"
            elif $v == ["NONE"] then empty
            elif ($v | index("NONE")) != null then "\($id): metadata.\($k) mixes NONE with ids"
            else ($v[] | select(type != "string" or (test($re) | not)) | "\($id): metadata.\($k) entry \(tojson) is not one of \($what)")
            end ),
        need([$qcs[] | select(deps("blocks") | index($s.id))] | length == 1; "\($id): needs exactly one sanity check bead blocked by it"),
        ( ($s | deps("blocks"))[] as $b
          | need(($b | endswith("-sanity")) or ($b | startswith("\($rid)-plan-qa")); "\($id): blocked by \($b), which is not a sanity check bead (a dev bead waits on its prerequisites' sanity checks)") ),
        ( ($s | deps("blocks"))[] | select(endswith("-sanity")) as $b | select([$all[] | select(.id == $b)] | length == 1)
          | need($qcdev[$b] != null; "\($id): blocked by \($b), which is in the plan but is not a sanity check bead") ),
        need(($g[$s.id] | closure($g) | index($s.id)) == null; "\($id): dependency cycle (it waits, through sanity checks, on its own work)"),
        [ ($s | deps("blocks"))[] | select(endswith("-sanity")) ] as $pre
        | ( if $m.relation == "must_follow" then need($pre | length > 0; "\($id): must_follow but blocked by no sanity check")
          elif $m.relation == "root" then need($pre | length == 0; "\($id): root but blocked by \($pre | join(", "))")
          else empty end ),
        ( if $m.layer == 1 then need($m.pr_target == $trunk; "\($id): layer 1 pr_target must be \($trunk)")
          else
            [ $devs[] | select(.metadata.stack == $m.stack and .metadata.layer == $m.layer - 1) ] as $below
            | if ($below | length) == 1 then need($m.pr_target == $below[0].metadata.branch; "\($id): pr_target must be \($below[0].metadata.branch) (layer below)")
              else empty end
          end ) ),
    ( $qcs[] as $q | "\($q.id)" as $id
      | need($q.assignee | blank | not; "\($id): no assignee (the dev-sanity agent)"),
        need(($q.metadata.dev_bead // "") == (($q | deps("blocks"))[0] // "-"); "\($id): metadata.dev_bead is not the dev bead that blocks it"),
        need(($q | deps("blocks")) | length == 1; "\($id): must be blocked by exactly one dev bead"),
        need(($q | deps("blocks"))[0] as $d | [$devs[] | select(.id == $d)] | length == 1; "\($id): its dev bead is not in this plan") ),
    ( $devs | group_by([.metadata.stack, .metadata.layer])[] | select(length > 1)
      | "stack \(.[0].metadata.stack) layer \(.[0].metadata.layer) claimed by \(map(.id) | join(", "))" )
  ]
| if length == 0 then "plan ok: \($devs | length) dev and \($qcs | length) sanity check beads under \($rid)" else (.[], ("\(length) problem(s)\n" | halt_error(5))) end
