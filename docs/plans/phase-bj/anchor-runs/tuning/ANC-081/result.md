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
              0.98,
              0.98
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
              0.82,
              0.85
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
              0.98,
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
      }
    ],
    "accepted": true,
    "authority_citations": [
      "ANC-081 v3 locked statement and fixture set",
      "phase-bj intent v1"
    ],
    "authority_recommendations": [],
    "candidate_rankings": [
      {
        "accepted": true,
        "candidate_id": "candidate-3",
        "exact_role_cells": 21,
        "exact_semantic_verdicts": 7,
        "question_chars": 2124,
        "question_sha256": "ec15d318755e9153b2b4841293b274d5eee51f62b62c5ae0b47f4569c55767f2",
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
        "question_chars": 2203,
        "question_sha256": "f4361e4cc057fb7e03bcf1a6802f138176eb2c7b4ea865e1c5c3409669d464b0",
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
        "exact_role_cells": 19,
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
      "mean": 0.9892857142857142,
      "median": 1.0,
      "minimum": 0.835,
      "p10": 0.99,
      "threshold": 0.8,
      "total_cells": 21
    },
    "confidence_reference_threshold": 0.8,
    "evidence": {
      "candidate_3_question_chars": 2124,
      "candidate_3_question_sha256": "ec15d318755e9153b2b4841293b274d5eee51f62b62c5ae0b47f4569c55767f2",
      "candidate_count": 3,
      "candidate_pool": [
        "candidate-1-default",
        "candidate-2",
        "candidate-3"
      ],
      "frozen_oracle": {
        "anchor_id": "ANC-081",
        "anchor_statement": "The difficulty value must be copied to every generated sprint-family dev, fix, sanity and QA bead at pour time.",
        "candidate_minimum": 3,
        "probes": [
          {
            "expected": {
              "addressed": "yes",
              "contradicted": "no",
              "relevant": "yes"
            },
            "id": "pass_exact",
            "text": "At pour time, copy the sprint container's difficulty value onto every generated sprint-family dev, fix, sanity, and QA bead.",
            "verdict": "satisfied",
            "why": "It explicitly copies the source sprint difficulty value to every named generated family bead at pour time."
          },
          {
            "expected": {
              "addressed": "yes",
              "contradicted": "no",
              "relevant": "yes"
            },
            "id": "pass_paraphrase",
            "text": "When pouring a sprint, propagate its difficulty field into each generated dev, fix, sanity, and QA task in that sprint family.",
            "verdict": "satisfied",
            "why": "'Propagate its difficulty field' directly paraphrases copying the sprint's difficulty value to every named family bead."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "yes",
              "relevant": "yes"
            },
            "id": "fail_fixture",
            "text": "The sprint-family pour writes no difficulty value onto its generated dev, fix, sanity or QA beads.",
            "verdict": "contradicted",
            "why": "It concerns pour-time difficulty assignment but explicitly plans the opposite of the required copy."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "no",
              "relevant": "no"
            },
            "id": "silence",
            "text": "Update the command-line README examples to use the current option names.",
            "verdict": "not_applicable",
            "why": "This concerns CLI documentation and neither follows nor conflicts with difficulty propagation during sprint-family pour."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "no",
              "relevant": "yes"
            },
            "id": "topical_partial",
            "text": "The sprint-family pour writes a difficulty value into generated beads.",
            "verdict": "not_addressed",
            "why": "It touches the same field and operation, but does not say to copy the sprint's difficulty or cover every dev/fix/sanity/QA bead."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "yes",
              "relevant": "yes"
            },
            "id": "direct_conflict",
            "text": "During pour, assign each generated dev, fix, sanity, and QA bead its own difficulty instead of copying the sprint container's difficulty.",
            "verdict": "contradicted",
            "why": "It explicitly rejects copying the sprint difficulty, even though it names all target bead kinds."
          },
          {
            "expected": {
              "addressed": "no",
              "contradicted": "yes",
              "relevant": "yes"
            },
            "id": "boundary_partial",
            "text": "At pour time, copy the sprint difficulty to every generated dev and QA bead; leave generated fix and sanity beads with their defaults.",
            "verdict": "contradicted",
            "why": "It copies the right source for two kinds but explicitly excludes two kinds required by the anchor."
          }
        ],
        "protocol": "plan-anchor/question-tournament/2.0.0",
        "repeat_count": 2
      },
      "frozen_oracle_sha256": "a2a4a7e34de4d54b3f1bd173015e90de430d9b91a45e47c0c3bbabaf2e9726b1",
      "request_repeat_pairs_identical": true,
      "run_ids": [
        "plan-20261019T001026Z-ANC-081-pass_exact",
        "plan-20261019T001026Z-ANC-081-pass_exact",
        "plan-20261019T001027Z-ANC-081-pass_paraphrase",
        "plan-20261019T001027Z-ANC-081-pass_paraphrase",
        "plan-20261019T001027Z-ANC-081-fail_fixture",
        "plan-20261019T001027Z-ANC-081-fail_fixture",
        "plan-20261019T001027Z-ANC-081-silence",
        "plan-20261019T001027Z-ANC-081-silence",
        "plan-20261019T001027Z-ANC-081-topical_partial",
        "plan-20261019T001027Z-ANC-081-topical_partial",
        "plan-20261019T001027Z-ANC-081-direct_conflict",
        "plan-20261019T001027Z-ANC-081-direct_conflict",
        "plan-20261019T001027Z-ANC-081-boundary_partial",
        "plan-20261019T001027Z-ANC-081-boundary_partial"
      ],
      "tournament_calls": 42
    },
    "explanation": "Candidate 3 is the shortest role-complete wording set that matched every frozen role answer and semantic verdict on all seven independent probes in both repeats. The default candidate failed the topical partial and boundary partial oracle cells; candidate 2 also passed, but used a longer question set. The frozen oracle uses “irrelevant” conceptually; the local scorer’s corresponding exact verdict label is “not_applicable.”",
    "gate": "planning",
    "operation": "tune",
    "oracle_sha256": "a2a4a7e34de4d54b3f1bd173015e90de430d9b91a45e47c0c3bbabaf2e9726b1",
    "planner_action": "Store the sealed plan question set as the ANC-081 plan override.",
    "production_confidence_threshold": 0.6,
    "production_semantics_unchanged": true,
    "question_action": null,
    "sealed_question_set": {
      "digest": "ec15d318755e9153b2b4841293b274d5eee51f62b62c5ae0b47f4569c55767f2",
      "questions": {
        "addressed": {
          "criteria": {
            "no": "The text does not include the complete source-to-all-targets requirement, including when it mentions a difficulty field only generally, leaves out a named bead kind, or gives another source.",
            "unclear": "The text suggests the sprint record's difficulty value is copied to the complete set of generated bead kinds, but leaves timing or recipients uncertain.",
            "yes": "The text states that every generated dev, fix, sanity, and QA bead receives the sprint record's difficulty value during pour, including by copy, inheritance, or propagation."
          },
          "instructions": "Does `item.text` require the pour to take the difficulty field from the sprint record and place that same value on each generated dev, fix, sanity, and QA bead? 'Copy', 'inherit', and 'propagate' can express the same operation. Judge presence only, not whether another clause conflicts.",
          "type": "choice"
        },
        "contradicted": {
          "criteria": {
            "no": "It does not explicitly choose an incompatible outcome; silence, general difficulty-field mentions, or omission of details are not contradictions.",
            "unclear": "The text gives conflicting or ambiguous directions, so whether the generated beads receive the sprint difficulty cannot be determined.",
            "yes": "The text explicitly assigns another difficulty source, rejects copying the sprint difficulty, or excludes a required bead kind from receiving that value."
          },
          "instructions": "Does `item.text` explicitly require an outcome incompatible with assigning the sprint record's difficulty value to every generated dev, fix, sanity, and QA bead during pour? Do not infer conflict from silence or a merely incomplete description.",
          "type": "choice"
        },
        "relevant": {
          "criteria": {
            "no": "The item concerns a different activity and does not affect difficulty assignment to generated sprint-family beads during pour.",
            "unclear": "The text is too vague to determine whether it concerns difficulty assignment to generated sprint-family beads during pour.",
            "yes": "The item plans, changes, or depends on difficulty assignment for one or more of those generated sprint-family beads during pour, whether it copies correctly, copies incompletely, or conflicts."
          },
          "instructions": "Does `item.text` concern assigning or preserving the difficulty field on generated dev, fix, sanity, or QA beads during an atm-bd-orchestration sprint-family pour? Judge subject overlap only, not compliance.",
          "type": "choice"
        }
      }
    },
    "sealed_question_set_digest_after": "ec15d318755e9153b2b4841293b274d5eee51f62b62c5ae0b47f4569c55767f2",
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
            "anchor_id": "ANC-081",
            "anchor_record_sha256": "3a9a6d23034c7da62bd684376117e204c336432b7ef7052d7ab6f1dc734f0d1f",
            "anchor_version": 3,
            "intent_id": "phase-bj",
            "intent_sha256": "e5d9e1e48df29bf874f8310256ea851ce17795770523139fbe56b4ea255c9382",
            "intent_version": 1
          },
          "candidate": {
            "candidate_id": "candidate-3",
            "question_sha256": "ec15d318755e9153b2b4841293b274d5eee51f62b62c5ae0b47f4569c55767f2"
          },
          "decision": {
            "question_set_sha256": "ec15d318755e9153b2b4841293b274d5eee51f62b62c5ae0b47f4569c55767f2",
            "reason_code": "TOURNAMENT.ACCEPTED",
            "status": "sealed"
          },
          "event": "question_set_sealed",
          "event_index": 1,
          "evidence": {
            "canonical_request_sha256": [
              "66aca185dc5d9d9c84aaa3da8fab0c656e29294b821d4c42ad74cf06edc0af9c",
              "66aca185dc5d9d9c84aaa3da8fab0c656e29294b821d4c42ad74cf06edc0af9c",
              "790e596681cc055d8e65eaf0fd53c91c9f15a76186c599e2dbbce2b0dfb26e6e",
              "790e596681cc055d8e65eaf0fd53c91c9f15a76186c599e2dbbce2b0dfb26e6e",
              "f2e79111bbffbcef40f4b26e4c2a749bc10f6145092d97b61805af399a84d802",
              "f2e79111bbffbcef40f4b26e4c2a749bc10f6145092d97b61805af399a84d802",
              "e62e8d2330026915d8ada950e66acfe9dd783ef74897b381d43bfea58e848a1d",
              "e62e8d2330026915d8ada950e66acfe9dd783ef74897b381d43bfea58e848a1d",
              "109416c63075477377f7c11d25d71aaa83acf25273bb5ee1796906ac180c32ce",
              "109416c63075477377f7c11d25d71aaa83acf25273bb5ee1796906ac180c32ce",
              "1c17d237d17f28c5bd016c489cfbe16cc8925d67b6e48f11469b8485c0f7a058",
              "1c17d237d17f28c5bd016c489cfbe16cc8925d67b6e48f11469b8485c0f7a058",
              "fbda2e9c5a6854a672c1ac30e1b2cab83e94704c788e87e168ad0e84c19aedc6",
              "fbda2e9c5a6854a672c1ac30e1b2cab83e94704c788e87e168ad0e84c19aedc6"
            ],
            "raw_response_sha256": [
              "feb1f3fa3a3477e9e6d52207b53b4194cb808658eb089dbc7bbb253fa54f66a1",
              "cafa3da30a3772ce541089bcec0111850f5bf9063bbdcd828c4c20123f62b2fe",
              "cafa3da30a3772ce541089bcec0111850f5bf9063bbdcd828c4c20123f62b2fe",
              "123c2faebbe5ec2da0da53817da2e96d0a9f02c575f1050f398bc3b71f4e0e4c",
              "955b4d394880c6663a08514eac946da4b7ed24152821f02926a1de4d5804bec9",
              "d530acd08ec68b92fbc976ff5aca5b3bc7ad880d2863bad9ca50ccb8e9d2b1c5",
              "d0f3e54d1d997da4c73d872b5fcebc71b029e1abf2873612023b39b1437ef192",
              "d0f3e54d1d997da4c73d872b5fcebc71b029e1abf2873612023b39b1437ef192",
              "7091383a6cb3f34dc80b3574cfe5792e6573cdde1a7bb1dc3d69ed8a15a8f9b7",
              "1ff8c9b1d9997b3ac7c455cf4dc04aa9ae136f38714f734e50913734a3a8c686",
              "bc9d9c9616d1770ad176a4170a666a922cd3905653619a26d14a6bb114792b33",
              "bc9d9c9616d1770ad176a4170a666a922cd3905653619a26d14a6bb114792b33",
              "bc9d9c9616d1770ad176a4170a666a922cd3905653619a26d14a6bb114792b33",
              "bc9d9c9616d1770ad176a4170a666a922cd3905653619a26d14a6bb114792b33"
            ],
            "run_ids": [
              "plan-20261019T001026Z-ANC-081-pass_exact",
              "plan-20261019T001026Z-ANC-081-pass_exact",
              "plan-20261019T001027Z-ANC-081-pass_paraphrase",
              "plan-20261019T001027Z-ANC-081-pass_paraphrase",
              "plan-20261019T001027Z-ANC-081-fail_fixture",
              "plan-20261019T001027Z-ANC-081-fail_fixture",
              "plan-20261019T001027Z-ANC-081-silence",
              "plan-20261019T001027Z-ANC-081-silence",
              "plan-20261019T001027Z-ANC-081-topical_partial",
              "plan-20261019T001027Z-ANC-081-topical_partial",
              "plan-20261019T001027Z-ANC-081-direct_conflict",
              "plan-20261019T001027Z-ANC-081-direct_conflict",
              "plan-20261019T001027Z-ANC-081-boundary_partial",
              "plan-20261019T001027Z-ANC-081-boundary_partial"
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
            "sha256": "a2a4a7e34de4d54b3f1bd173015e90de430d9b91a45e47c0c3bbabaf2e9726b1"
          },
          "previous_event_sha256": null,
          "protocol_version": "plan-anchor/question-tournament/2.0.0",
          "score": {
            "confidence": {
              "cells_at_or_above_threshold": 21,
              "mean": 0.9892857142857142,
              "median": 1.0,
              "minimum": 0.835,
              "p10": 0.99,
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
      "history_id": "ANC-081-plan",
      "schema": "plan-anchor/tuning-history/2"
    },
    "tuning_history_after_sha256": "5860dc8ed2a22bdd504ee7f7eddae3f15840ba62419c844faaca832b493d1a1f",
    "tuning_history_before_sha256": "4a038b895c5ae9344a1fabe4a55b80f68a110dfa54c0e49569623629c3dfc004"
  },
  "error": null,
  "metadata": {
    "authoritative_sources_unchanged": true,
    "correlation_id": "ANC-081",
    "duration_ms": 0,
    "jev_run_ids": [
      "plan-20261019T001026Z-ANC-081-pass_exact",
      "plan-20261019T001026Z-ANC-081-pass_exact",
      "plan-20261019T001027Z-ANC-081-pass_paraphrase",
      "plan-20261019T001027Z-ANC-081-pass_paraphrase",
      "plan-20261019T001027Z-ANC-081-fail_fixture",
      "plan-20261019T001027Z-ANC-081-fail_fixture",
      "plan-20261019T001027Z-ANC-081-silence",
      "plan-20261019T001027Z-ANC-081-silence",
      "plan-20261019T001027Z-ANC-081-topical_partial",
      "plan-20261019T001027Z-ANC-081-topical_partial",
      "plan-20261019T001027Z-ANC-081-direct_conflict",
      "plan-20261019T001027Z-ANC-081-direct_conflict",
      "plan-20261019T001027Z-ANC-081-boundary_partial",
      "plan-20261019T001027Z-ANC-081-boundary_partial"
    ],
    "planner_observation_trust": null,
    "retry_count": 0,
    "tool_calls": 42
  },
  "schema_version": "1.0.0",
  "success": true
}
```
