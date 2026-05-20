| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | findings_hash | supersedes | Note |
|-------|------|----------|-----------------|--------|----------|-----------|-------|---------------|------------|------|
| STEP1-R1 | 2 | plan-scope-reviewer | abc1234 | FAIL | 2 | 1 | 0 | hash-001 |  | initial scope review |
| STEP1-R2 | 2 | plan-scope-reviewer | def5678 | PASS | 0 | 0 | 1 | hash-002 | STEP1-R1 | findings closed |
| STEP3-R1 | 4 | critical-plan-reviewer | 1122aaa | FAIL | 1 | 2 | 1 | hash-101 |  | initial critical review |
| STEP3-R2 | 4 | critical-plan-reviewer | 3344bbb | PASS | 0 | 0 | 1 | hash-102 | STEP3-R1 | ready for QA handoff |
