# ANC-012 v3 invalidation

Rand approved the replacement statement for `--latest` on 2026-10-19. ANC-012 was updated from version 2 to version 3 with its verbatim quote unchanged. Every earlier ANC-012 gate result is invalid for the v3 requirement, even if the rest of its historical run remains useful evidence. Do not count such a cell as current coverage; rerun the affected cell or mark it stale. Existing run files are preserved as historical evidence.

`jev_judge.py coverage` selects the latest run per item without checking `anchor_version`. Its output alone must therefore not be presented as current ANC-012 coverage until the relevant cells have been rerun against v3.

The v3 live rubric was inconclusive on observable, faithful and self_contained; both fixture checks passed. The separate rubric and fixture JSON files record those outcomes. This note does not treat rubric uncertainty as permission to alter Rand's exact ruling.

Lead ruling on 2026-10-19: v3 remains locked on Rand's explicit approval. The rubric answered yes on all five dimensions, though three fell below the confidence threshold. The lead attributed the low confidence to the terse original quote and reported that three clearer rewordings did not resolve the faithful score. The approved statement was not changed again.
