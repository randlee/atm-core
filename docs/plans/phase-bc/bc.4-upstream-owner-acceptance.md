# bc.4 upstream owner acceptance

Status: historical PR106 evidence; u4 closed by PR #1571 (`b381e0981`) at
sc-publish main `f178b6919881c5a3d030d6343fcbb509f04806cc`.

## historical source — superseded for u4

- repository: `randlee/sc-publish`
- combined-stack PR: [#106](https://github.com/randlee/sc-publish/pull/106)
- historical head: `22137c2da13bf4638b4267b69c6c2f021617da73`
- consumer: `randlee/atm-core`
- acceptance scope: the shared publish-kit correction stack and its
  byte-for-byte consumer installer output

PR #101 remains historical evidence for the earlier immutable-release review;
the PR106-derived head above is frozen and is not the current bc.4 pin or u4
qualification. The active merged sc-publish stack is PRs #99/#100/#101/#108;
develop head `98a75ba3e` was reconciled with main (#98/#111) and promoted by
#110; PR #1571 pinned and reinstalled the #110 merge commit, closing u4.

The owner-approved sequencing amendment dated 2026-09-23 records that bc.4
local adoption proceeded before u2 and u3. u2 credential/setting activation
was not passed as a bc.4 gate and belongs to separately authorized bc.5. u3's
broad draft-first, tag-binding, concurrency, and digest/checksum expansion was
removed from the accepted sc-publish scope and is not claimed.

Do not block or ask about tokens unless preflight or publish fails.

## handoff

The sc-publish maintainer handed off historical PR106 evidence. bc.4 may own
consumer adoption only after the clean merged-develop pin/install evidence is
available; this document does not claim that qualification. bc.5 owns later
preflight and release evidence. Inherited or deferred FIX03–FIX10 work is not
claimed by this handoff.
