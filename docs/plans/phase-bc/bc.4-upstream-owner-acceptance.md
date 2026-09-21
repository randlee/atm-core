# bc.4 upstream owner acceptance

## accepted source

- repository: `randlee/sc-publish`
- combined-stack PR: [#106](https://github.com/randlee/sc-publish/pull/106)
- exact accepted head: `22137c2da13bf4638b4267b69c6c2f021617da73`
- consumer: `randlee/atm-core`
- acceptance scope: the shared publish-kit correction stack and its
  byte-for-byte consumer installer output

PR #101 remains historical evidence for the earlier immutable-release review;
it is not the bc.4 pin or an additional entry gate. Repository setting and
credential activation are separately authorized bc.5 work.

Do not block or ask about tokens unless preflight or publish fails.

## handoff

The sc-publish maintainer handed off the exact source revision and the combined
stack's correction evidence. bc.4 owns consumer adoption and qualification;
bc.5 owns any later setting or credential activation. Inherited or deferred
FIX03–FIX10 work is not claimed by this handoff.
