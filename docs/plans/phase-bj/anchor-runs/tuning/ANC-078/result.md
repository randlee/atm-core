```json
{
  "aborted_by": null,
  "canceled": false,
  "data": {
    "acceptance_table": [
      {
        "case_id": "pass_exact",
        "expected_semantic_verdict": "satisfied",
        "observed_semantic_verdicts": [
          "satisfied",
          "satisfied"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              0.98,
              0.97
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      },
      {
        "case_id": "pass_paraphrase",
        "expected_semantic_verdict": "satisfied",
        "observed_semantic_verdicts": [
          "satisfied",
          "satisfied"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      },
      {
        "case_id": "fail_fixture",
        "expected_semantic_verdict": "contradicted",
        "observed_semantic_verdicts": [
          "contradicted",
          "contradicted"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              0.92,
              0.95
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              0.88,
              0.89
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      },
      {
        "case_id": "silence",
        "expected_semantic_verdict": "not_applicable",
        "observed_semantic_verdicts": [
          "not_applicable",
          "not_applicable"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      },
      {
        "case_id": "topical_partial",
        "expected_semantic_verdict": "not_addressed",
        "observed_semantic_verdicts": [
          "not_addressed",
          "not_addressed"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              0.94,
              0.97
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              0.98,
              0.98
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      },
      {
        "case_id": "direct_conflict",
        "expected_semantic_verdict": "contradicted",
        "observed_semantic_verdicts": [
          "contradicted",
          "contradicted"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      },
      {
        "case_id": "boundary_partial",
        "expected_semantic_verdict": "contradicted",
        "observed_semantic_verdicts": [
          "contradicted",
          "contradicted"
        ],
        "repeat_requests_byte_identical": true,
        "roles": [
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "no",
            "observed": [
              "no",
              "no"
            ],
            "repeat_stable": true,
            "role": "addressed"
          },
          {
            "confidence": [
              0.98,
              0.98
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "contradicted"
          },
          {
            "confidence": [
              1.0,
              1.0
            ],
            "exact": true,
            "expected": "yes",
            "observed": [
              "yes",
              "yes"
            ],
            "repeat_stable": true,
            "role": "relevant"
          }
        ],
        "semantic_exact": true,
        "stable": true
      }
    ],
    "accepted": true,
    "authority_citations": [
      "ANC-078 v3 locked statement and fixture set",
      "phase-bj intent v1"
    ],
    "authority_recommendations": [],
    "candidate_rankings": [
      {
        "accepted": true,
        "candidate_id": "candidate-3",
        "exact_role_cells": 21,
        "exact_semantic_verdicts": 7,
        "question_chars": 2099,
        "question_sha256": "01bb508e0ee98cd887555ca079d1d7f67cbded354ad2f9a1363bfe36e5c88f05",
        "rank": 1,
        "regressions": 0,
        "stable_cases": 7,
        "total_cases": 7,
        "total_role_cells": 21,
        "total_semantic_verdicts": 7
      },
      {
        "accepted": true,
        "candidate_id": "candidate-2",
        "exact_role_cells": 21,
        "exact_semantic_verdicts": 7,
        "question_chars": 2160,
        "question_sha256": "4c589da000837ddd895560774535e346a31a14638325956d87773470da979164",
        "rank": 2,
        "regressions": 0,
        "stable_cases": 7,
        "total_cases": 7,
        "total_role_cells": 21,
        "total_semantic_verdicts": 7
      },
      {
        "accepted": false,
        "candidate_id": "candidate-1-default",
        "exact_role_cells": 18,
        "exact_semantic_verdicts": 5,
        "question_chars": 2278,
        "question_sha256": "8e9e29b043185ff7a76912e1d28726222bcd3fc867dcabb78a9bef523029ec05",
        "rank": 3,
        "regressions": 2,
        "stable_cases": 7,
        "total_cases": 7,
        "total_role_cells": 21,
        "total_semantic_verdicts": 7
      }
    ],
    "confidence_diagnostics": {
      "cells_at_or_above_threshold": 21,
      "mean": 0.9861904761904763,
      "median": 1.0,
      "minimum": 0.885,
      "p10": 0.955,
      "threshold": 0.8,
      "total_cells": 21
    },
    "confidence_reference_threshold": 0.8,
    "evidence": {
      "candidate_3_question_chars": 2099,
      "candidate_3_question_sha256": "01bb508e0ee98cd887555ca079d1d7f67cbded354ad2f9a1363bfe36e5c88f05",
      "candidate_count": 3,
      "candidate_pool": [
        "candidate-1-default",
        "candidate-2",
        "candidate-3"
      ],
      "frozen_oracle": {
        "anchor_id": "ANC-078",
        "anchor_statement": "The branch value must be copied to every generated sprint-family dev, fix, sanity and QA bead at pour time.",
        "candidate_minimum": 3,
        "probes": [
          {
            "expected": {
              "addressed": "yes",
              "contradicted": "no",
              "relevant": "yes"
            },
            "id": "pass_exact",
            "text": "At pour time, copy the sprint container's branch value onto every generated sprint-family dev, fix, sanity, and QA bead.",
            "verdict": "satisfied",
            "why": "It explicitly copies the source sprint branch to every named generated family bead at pour time."
          },
          {
            "expected": {
              "addressed": "yes",
              "contradicted": "no",
              "relevant": "yes"
            },
            "id": "pass_paraphrase",
            "text": "When pouring a sprint, propagate its branch field into each generated dev, fix, sanity, and QA task in that sprint family.",
            "verdict": "satisfied",
            "why": "'Propagate its branch field' is a direct paraphrase of copying the sprint's branch value to every named family bead."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "yes",
              "relevant": "yes"
            },
            "id": "fail_fixture",
            "text": "The sprint-family pour writes no branch value onto its generated dev, fix, sanity or QA beads.",
            "verdict": "contradicted",
            "why": "It concerns the pour's branch assignment but explicitly plans the opposite of the required copy."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "no",
              "relevant": "no"
            },
            "id": "silence",
            "text": "Update the command-line README examples to use the current option names.",
            "verdict": "irrelevant",
            "why": "This is documentation for CLI examples and neither follows nor conflicts with branch propagation during sprint-family pour."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "no",
              "relevant": "yes"
            },
            "id": "topical_partial",
            "text": "The sprint-family pour writes a branch value into generated beads.",
            "verdict": "not_addressed",
            "why": "It touches the same field and operation, but does not say to copy the sprint's branch or cover every dev/fix/sanity/QA bead."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "yes",
              "relevant": "yes"
            },
            "id": "direct_conflict",
            "text": "During pour, assign each generated dev, fix, sanity, and QA bead its own branch instead of copying the sprint container's branch.",
            "verdict": "contradicted",
            "why": "It explicitly rejects copying the sprint branch, even though it names all target bead kinds."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "yes",
              "relevant": "yes"
            },
            "id": "boundary_partial",
            "text": "At pour time, copy the sprint branch to every generated dev and QA bead; leave generated fix and sanity beads with their defaults.",
            "verdict": "contradicted",
            "why": "It copies the right source for two kinds but explicitly excludes two kinds the anchor requires."
          }
        ],
        "protocol": "plan-anchor/question-tournament/2.0.0",
        "repeat_count": 2
      },
      "frozen_oracle_sha256": "ea94245fedecb62a46d4bddc7a74c84d515492e06b449d30d0fda2b7c513468d",
      "request_repeat_pairs_identical": true,
      "run_ids": [
        "plan-20261019T000527Z-ANC-078-pass_exact",
        "plan-20261019T000527Z-ANC-078-pass_exact",
        "plan-20261019T000527Z-ANC-078-pass_paraphrase",
        "plan-20261019T000527Z-ANC-078-pass_paraphrase",
        "plan-20261019T000527Z-ANC-078-fail_fixture",
        "plan-20261019T000527Z-ANC-078-fail_fixture",
        "plan-20261019T000527Z-ANC-078-silence",
        "plan-20261019T000527Z-ANC-078-silence",
        "plan-20261019T000527Z-ANC-078-topical_partial",
        "plan-20261019T000527Z-ANC-078-topical_partial",
        "plan-20261019T000527Z-ANC-078-direct_conflict",
        "plan-20261019T000527Z-ANC-078-direct_conflict",
        "plan-20261019T000527Z-ANC-078-boundary_partial",
        "plan-20261019T000527Z-ANC-078-boundary_partial"
      ],
      "tournament_calls": 42
    },
    "explanation": "Candidate 3 is the shortest role-complete wording set that matched every frozen role answer and semantic verdict on all seven independent probes in both repeats. The default candidate failed the topical partial and boundary partial oracle cells; candidate 2 also passed, but used a longer question set. The frozen oracle uses “irrelevant” conceptually; the local scorer’s corresponding exact verdict label is “not_applicable.”",
    "gate": "planning",
    "operation": "tune",
    "oracle_sha256": "ea94245fedecb62a46d4bddc7a74c84d515492e06b449d30d0fda2b7c513468d",
    "planner_action": "Store the sealed plan question set as the ANC-078 plan override.",
    "production_confidence_threshold": 0.6,
    "production_semantics_unchanged": true,
    "question_action": null,
    "sealed_question_set": {
      "digest": "01bb508e0ee98cd887555ca079d1d7f67cbded354ad2f9a1363bfe36e5c88f05",
      "questions": {
        "addressed": {
          "criteria": {
            "no": "The text does not include the complete source-to-all-targets requirement, including when it mentions a branch field only generally, leaves out a named bead kind, or gives another source.",
            "unclear": "The text suggests that the sprint record's branch value is copied to the complete set of generated bead kinds, but leaves timing or the set of recipients uncertain.",
            "yes": "The text states that every generated dev, fix, sanity, and QA bead receives the sprint record's branch value during pour, including by copy, inheritance, or propagation."
          },
          "instructions": "Does `item.text` require the pour to take the branch field from the sprint record and place that same value on each generated dev, fix, sanity, and QA bead? 'Copy', 'inherit', and 'propagate' can express the same operation. Judge presence only, not whether another clause conflicts.",
          "type": "choice"
        },
        "contradicted": {
          "criteria": {
            "no": "It does not explicitly choose an incompatible outcome; silence, general branch-field mentions, or omission of details are not contradictions.",
            "unclear": "It gives conflicting or ambiguous directions, so whether the generated beads receive the sprint branch cannot be determined.",
            "yes": "The text explicitly assigns another branch source, rejects copying the sprint branch, or excludes a required bead kind from receiving that branch value."
          },
          "instructions": "Does `item.text` explicitly require an outcome incompatible with assigning the sprint record's branch value to every generated dev, fix, sanity, and QA bead during pour? Do not infer conflict from silence or a merely incomplete description.",
          "type": "choice"
        },
        "relevant": {
          "criteria": {
            "no": "The item concerns a different activity and does not affect branch assignment to generated sprint-family beads during pour.",
            "unclear": "The text is too vague to determine whether it concerns branch assignment to generated sprint-family beads during pour.",
            "yes": "The item plans, changes, or depends on branch assignment for one or more of those generated sprint-family beads during pour, whether it copies correctly, copies incompletely, or conflicts."
          },
          "instructions": "Does `item.text` concern assigning or preserving the branch field on generated dev, fix, sanity, or QA beads during an atm-bd-orchestration sprint-family pour? Judge only overlap with this subject, not compliance.",
          "type": "choice"
        }
      }
    },
    "sealed_question_set_digest_after": "01bb508e0ee98cd887555ca079d1d7f67cbded354ad2f9a1363bfe36e5c88f05",
    "sealed_question_set_digest_before": null,
    "semantic_score": {
      "exact_role_cells": 21,
      "exact_semantic_verdicts": 7,
      "regressions": 0,
      "stable_cases": 7,
      "total_cases": 7,
      "total_role_cells": 21,
      "total_semantic_verdicts": 7
    },
    "tournament_protocol_version": "plan-anchor/question-tournament/2.0.0",
    "tuning_history_after": {
      "events": [
        {
          "authority": {
            "anchor_id": "ANC-078",
            "anchor_record_sha256": "570e3f028a03e4fc12e5a45c5eee9de74c58ba9974bdd2930f978b20c5d82389",
            "anchor_version": 3,
            "intent_id": "phase-bj",
            "intent_sha256": "e5d9e1e48df29bf874f8310256ea851ce17795770523139fbe56b4ea255c9382",
            "intent_version": 1
          },
          "candidate": {
            "candidate_id": "candidate-3",
            "question_sha256": "01bb508e0ee98cd887555ca079d1d7f67cbded354ad2f9a1363bfe36e5c88f05"
          },
          "decision": {
            "question_set_sha256": "01bb508e0ee98cd887555ca079d1d7f67cbded354ad2f9a1363bfe36e5c88f05",
            "reason_code": "TOURNAMENT.ACCEPTED",
            "status": "sealed"
          },
          "event": "question_set_sealed",
          "event_index": 1,
          "evidence": {
            "canonical_request_sha256": [
              "1f1026164605b80624329e54d9651f17b72fd1aa4742b5fe23a8065417ffb2d3",
              "1f1026164605b80624329e54d9651f17b72fd1aa4742b5fe23a8065417ffb2d3",
              "fbaae9738f9e6dd484859628da0253a276f21c2798022c744441886f85fcf84a",
              "fbaae9738f9e6dd484859628da0253a276f21c2798022c744441886f85fcf84a",
              "81cde3f11a5c744155bad0a537500cf5a6d242b77b08fd19feb3e5002d9f79c6",
              "81cde3f11a5c744155bad0a537500cf5a6d242b77b08fd19feb3e5002d9f79c6",
              "64f986ea594b20cf97684d5e6a4728cb71650968e3548528a68986c5fa7a5dcb",
              "64f986ea594b20cf97684d5e6a4728cb71650968e3548528a68986c5fa7a5dcb",
              "fe9f578b1e9003e82e79562349528dcff0531193ed673462a319651aba84ac07",
              "fe9f578b1e9003e82e79562349528dcff0531193ed673462a319651aba84ac07",
              "d5c40f1159e6e6be1170839ee67a90e6a66972785aa4b3ee641528c323ce4f98",
              "d5c40f1159e6e6be1170839ee67a90e6a66972785aa4b3ee641528c323ce4f98",
              "c20d26c5622ce6466c1e7a63b8012a4f3b9d67f20bc5262b31f0e7da09d2d943",
              "c20d26c5622ce6466c1e7a63b8012a4f3b9d67f20bc5262b31f0e7da09d2d943"
            ],
            "raw_response_sha256": [
              "feb1f3fa3a3477e9e6d52207b53b4194cb808658eb089dbc7bbb253fa54f66a1",
              "9f6f9410c182630054f4241e7aa5ae0b7555e606a5128ca1da66231815b6fbb8",
              "cafa3da30a3772ce541089bcec0111850f5bf9063bbdcd828c4c20123f62b2fe",
              "cafa3da30a3772ce541089bcec0111850f5bf9063bbdcd828c4c20123f62b2fe",
              "968abbb4e0681462dd738351456a4f8f83f08efbea7c698b47df8c29fab996d1",
              "ababeb07badc5ed4805820c7ceee6c8be132b6ace6dd0693f05c6de45a629cba",
              "d0f3e54d1d997da4c73d872b5fcebc71b029e1abf2873612023b39b1437ef192",
              "d0f3e54d1d997da4c73d872b5fcebc71b029e1abf2873612023b39b1437ef192",
              "bb926f4ae4ba428805fa53c3abb061bb7262cc83d2925f4ead767ec87d5f48f6",
              "a4e6b7ac497c062523792d0715722732afcf4ded685a6717df78b18242463968",
              "bc9d9c9616d1770ad176a4170a666a922cd3905653619a26d14a6bb114792b33",
              "bc9d9c9616d1770ad176a4170a666a922cd3905653619a26d14a6bb114792b33",
              "5574c3ac8dcb16758d763c14778452821bd10ba436beefe4052bcecf967b4f10",
              "5574c3ac8dcb16758d763c14778452821bd10ba436beefe4052bcecf967b4f10"
            ],
            "run_ids": [
              "plan-20261019T000527Z-ANC-078-pass_exact",
              "plan-20261019T000527Z-ANC-078-pass_exact",
              "plan-20261019T000527Z-ANC-078-pass_paraphrase",
              "plan-20261019T000527Z-ANC-078-pass_paraphrase",
              "plan-20261019T000527Z-ANC-078-fail_fixture",
              "plan-20261019T000527Z-ANC-078-fail_fixture",
              "plan-20261019T000527Z-ANC-078-silence",
              "plan-20261019T000527Z-ANC-078-silence",
              "plan-20261019T000527Z-ANC-078-topical_partial",
              "plan-20261019T000527Z-ANC-078-topical_partial",
              "plan-20261019T000527Z-ANC-078-direct_conflict",
              "plan-20261019T000527Z-ANC-078-direct_conflict",
              "plan-20261019T000527Z-ANC-078-boundary_partial",
              "plan-20261019T000527Z-ANC-078-boundary_partial"
            ]
          },
          "oracle": {
            "case_count": 7,
            "case_ids": [
              "pass_exact",
              "pass_paraphrase",
              "fail_fixture",
              "silence",
              "topical_partial",
              "direct_conflict",
              "boundary_partial"
            ],
            "repeat_count": 2,
            "sha256": "ea94245fedecb62a46d4bddc7a74c84d515492e06b449d30d0fda2b7c513468d"
          },
          "previous_event_sha256": null,
          "protocol_version": "plan-anchor/question-tournament/2.0.0",
          "score": {
            "confidence": {
              "cells_at_or_above_threshold": 21,
              "mean": 0.9861904761904763,
              "median": 1.0,
              "minimum": 0.885,
              "p10": 0.955,
              "threshold": 0.8,
              "total_cells": 21
            },
            "exact_role_cells": 21,
            "exact_semantic_verdicts": 7,
            "regressions": 0,
            "stable_cases": 7,
            "total_cases": 7,
            "total_role_cells": 21,
            "total_semantic_verdicts": 7
          },
          "supersedes": null
        }
      ],
      "history_id": "ANC-078-plan",
      "schema": "plan-anchor/tuning-history/2"
    },
    "tuning_history_after_sha256": "cc0268071e037e5f1d6927344c8dfe607fcd1785aa8e789b35d5ef8e025739c1",
    "tuning_history_before_sha256": "229307250c25ee4f544bcad48b2558c4a2e3d7b38354c18cab1365ac5a11276e"
  },
  "error": null,
  "metadata": {
    "authoritative_sources_unchanged": true,
    "correlation_id": "ANC-078",
    "duration_ms": 0,
    "jev_run_ids": [
      "plan-20261019T000527Z-ANC-078-pass_exact",
      "plan-20261019T000527Z-ANC-078-pass_exact",
      "plan-20261019T000527Z-ANC-078-pass_paraphrase",
      "plan-20261019T000527Z-ANC-078-pass_paraphrase",
      "plan-20261019T000527Z-ANC-078-fail_fixture",
      "plan-20261019T000527Z-ANC-078-fail_fixture",
      "plan-20261019T000527Z-ANC-078-silence",
      "plan-20261019T000527Z-ANC-078-silence",
      "plan-20261019T000527Z-ANC-078-topical_partial",
      "plan-20261019T000527Z-ANC-078-topical_partial",
      "plan-20261019T000527Z-ANC-078-direct_conflict",
      "plan-20261019T000527Z-ANC-078-direct_conflict",
      "plan-20261019T000527Z-ANC-078-boundary_partial",
      "plan-20261019T000527Z-ANC-078-boundary_partial"
    ],
    "planner_observation_trust": null,
    "retry_count": 0,
    "tool_calls": 42
  },
  "schema_version": "1.0.0",
  "success": true
}
```
