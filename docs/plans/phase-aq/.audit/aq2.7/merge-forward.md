# AQ2.7 merge-forward record

Before QA-2 implementation, the branch was current with its remote and then
merged the requested upstream branches using merge commits:

```text
git pull --no-rebase origin feature/aq-2-7-herdr-queue-wake
git fetch origin integrate/phase-aq feature/aq-1-6-graft-receiver-registration-client
git merge --no-edit origin/integrate/phase-aq
git merge --no-edit origin/feature/aq-1-6-graft-receiver-registration-client
```

The AQ1.6 merge had conflicts in the graft boundary files listed in
`qa2-validation.md`; they were resolved in merge commit `4a8bd241d` and
pushed immediately. No rebase or force push was used.
