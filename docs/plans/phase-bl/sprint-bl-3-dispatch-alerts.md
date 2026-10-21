# Sprint bl-3 — dispatch and bead-source held rows

closure_type `boundary`; target_boundary `BOUNDARY-DispatchHeld`. Branch
`sprint/bl-3-dispatch-alerts`, PR → `sprint/bl-2-jev-client`. Wave 1; no
consumed contract. `external_ref`: `gh-375,gh-372,gh-359`.

## Boundary and scope

This sprint owns dispatch behavior and bead-source data only. It does not own
console models, GitHub source rows, `atm_bd.py`, labels, or rendering; those
are bl-10. It has no dependency on Jev, refresh, or reviewer data.

Owned paths:

- `src/atm_bd_console/dispatch.py`
- `src/atm_bd_console/beads_source.py`
- `tests/test_dispatch.py`, `tests/test_beads_source.py`, and new
  `tests/test_dispatch_held.py`

`BOUNDARY-DispatchHeld` is the typed list of held dispatch/bead rows:
`bead_id`, `task_id`, `reason`, and source timestamp. Its consumers may read
that value without editing these files.

## Deliverables and deterministic criteria

1. Represent a held dispatch decision as a typed row and preserve its bead
   and task identifiers through the bead-source read.
2. Render-ready dispatch data carries a stable reason for unavailable or held
   work; it never requires a model or GitHub source edit.
3. Verify with controlled source records that a held task produces one typed
   row, an absent task produces none, and repeated identical input produces
   byte-identical rows.

## Does not close

#491 label/PR presentation and #359 model text belong to bl-10. Notification
or dispatch timing is not changed here.
