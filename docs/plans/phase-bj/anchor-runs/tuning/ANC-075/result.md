```json
{
  "schema_version": "1.0.0",
  "success": true,
  "canceled": false,
  "aborted_by": null,
  "data": {
    "operation": "tune",
    "gate": "planning",
    "accepted": true,
    "tournament_protocol_version": "plan-anchor/question-tournament/2.0.0",
    "production_confidence_threshold": 0.6,
    "confidence_reference_threshold": 0.8,
    "semantic_score": {
      "exact_role_cells": 48,
      "total_role_cells": 48,
      "exact_semantic_verdicts": 16,
      "total_semantic_verdicts": 16,
      "stable_cases": 8,
      "total_cases": 8,
      "regressions": 0
    },
    "confidence_diagnostics": {
      "threshold": 0.8,
      "cells_at_or_above_threshold": 46,
      "total_cells": 48,
      "minimum": 0.64,
      "p10": 0.97,
      "median": 1.0,
      "mean": 0.98
    },
    "tuning_history_before_sha256": "fcf0a84b56b4d7ea3f102ff5b31779e091c4f42ea4964e9ad359564ce7cb6280",
    "tuning_history_after": {
      "schema": "plan-anchor/tuning-history/2",
      "history_id": "ANC-075-plan",
      "events": [
        {
          "event_index": 1,
          "previous_event_sha256": null,
          "event": "question_set_sealed",
          "protocol_version": "plan-anchor/question-tournament/2.0.0",
          "authority": {
            "intent_id": "phase-bj",
            "intent_version": 1,
            "intent_sha256": "e5d9e1e48df29bf874f8310256ea851ce17795770523139fbe56b4ea255c9382",
            "anchor_id": "ANC-075",
            "anchor_version": 2,
            "anchor_record_sha256": "a52206c79531b87877cc59f28f2f1a3d43b520535b81b9bc6ddf794202d14a21"
          },
          "oracle": {
            "sha256": "8fa19a1aa07ef2bb4cd8bf97bd56ae2b0bc90913ea3dda01647529d6456be518",
            "case_ids": [
              "fixture_pass",
              "fixture_fail",
              "paraphrase",
              "silent",
              "topical",
              "conflict",
              "irrelevant",
              "boundary"
            ],
            "case_count": 8,
            "repeat_count": 2
          },
          "candidate": {
            "candidate_id": "output",
            "question_sha256": "1aa0372012b96a05b252004cc38c88e207bbddf83df3029e0faf3678663e46eb"
          },
          "evidence": {
            "canonical_request_sha256": [
              "1eeb797fdceae45de9e36ec04bb6adfc933ddd33e2cac0f4dd3da4234faf6407",
              "1eeb797fdceae45de9e36ec04bb6adfc933ddd33e2cac0f4dd3da4234faf6407",
              "17987fa8712e01b4c6cda09d941421baa277694b80eee9927677b6899f4384cc",
              "17987fa8712e01b4c6cda09d941421baa277694b80eee9927677b6899f4384cc",
              "f9166037f166e451bf023db1ad140a22433d29fd5d69229cca784e82c7460ece",
              "f9166037f166e451bf023db1ad140a22433d29fd5d69229cca784e82c7460ece",
              "48adbb87a784c1662eb7db0a78a8c63b9c82ee284b4a1a4036f0efbd5dd9e181",
              "48adbb87a784c1662eb7db0a78a8c63b9c82ee284b4a1a4036f0efbd5dd9e181",
              "af14df790d139b95c4a75e931fdd12a446b437105198df318d74709f022410b0",
              "af14df790d139b95c4a75e931fdd12a446b437105198df318d74709f022410b0",
              "edf8aaf8c631a16fdca37770dc49dcdc2a31a9a650eddb2155b38552ce3938fd",
              "edf8aaf8c631a16fdca37770dc49dcdc2a31a9a650eddb2155b38552ce3938fd",
              "766e77857cf0693ed90c457cfc58d7816f522946a955f3e30f324d722f1fded8",
              "766e77857cf0693ed90c457cfc58d7816f522946a955f3e30f324d722f1fded8",
              "2a58e6252bd4c350f3685e67d35b1f77f520bd282861fd33a2d48a6093032186",
              "2a58e6252bd4c350f3685e67d35b1f77f520bd282861fd33a2d48a6093032186",
              "f054a4c0020eacde75feeabe0f4163ea73fa714b95c90429e5fe2fa0ee5cdb4b",
              "f054a4c0020eacde75feeabe0f4163ea73fa714b95c90429e5fe2fa0ee5cdb4b",
              "e4c9f036721c3020cada51c525affa8be4a71651ff6e26b7fde39bd56a766f7b",
              "e4c9f036721c3020cada51c525affa8be4a71651ff6e26b7fde39bd56a766f7b",
              "cb4be84a8116946a60d5f5d423b8ab1722428ffd8f95cb8216ab07dd0d7eb82a",
              "cb4be84a8116946a60d5f5d423b8ab1722428ffd8f95cb8216ab07dd0d7eb82a",
              "0dc08a52527467a3dff70070bd03deee68f72ca57a0b8fafafcaab0761b953cf",
              "0dc08a52527467a3dff70070bd03deee68f72ca57a0b8fafafcaab0761b953cf",
              "eaba2b2b3a78ca7ed4487aa045d54e4618d539b4ee6ebd8cda5f55775bec3c6a",
              "eaba2b2b3a78ca7ed4487aa045d54e4618d539b4ee6ebd8cda5f55775bec3c6a",
              "a125cb40df8a558f9076b51459e71134b2d9f04d68e7437495bf5920df2d21d5",
              "a125cb40df8a558f9076b51459e71134b2d9f04d68e7437495bf5920df2d21d5",
              "46b00754419d2f1ad7ff08b5502be4bacfc23fb8df5b04974b814a076069cb50",
              "46b00754419d2f1ad7ff08b5502be4bacfc23fb8df5b04974b814a076069cb50",
              "67d543dd73c21717b91e32cf8f2efcc57a7cea3d39d4f8c17117ccf981f07cf2",
              "67d543dd73c21717b91e32cf8f2efcc57a7cea3d39d4f8c17117ccf981f07cf2",
              "b6b7984d31225ea6cd2c9f8ecbd54c66b9d071d93c3436ab47b419d8f4bfc362",
              "b6b7984d31225ea6cd2c9f8ecbd54c66b9d071d93c3436ab47b419d8f4bfc362",
              "2f4c1d9f93f75290be1fb6002cd8ff70e71c82fc98c325e15bde62b349016cc3",
              "2f4c1d9f93f75290be1fb6002cd8ff70e71c82fc98c325e15bde62b349016cc3",
              "9c1134ca53251e673b1ce1888d83c7fe04561e5d408c60b64296c2381fb1896c",
              "9c1134ca53251e673b1ce1888d83c7fe04561e5d408c60b64296c2381fb1896c",
              "ec2b67622fb3eecdbc5f5c9fbdd62d412dbe519dd8758bca2f6e67a26fee84fe",
              "ec2b67622fb3eecdbc5f5c9fbdd62d412dbe519dd8758bca2f6e67a26fee84fe",
              "2b8a045c77b7d5d34b29a5f3fceca83f6ed7ceffa06575deb8263dbbb49f8885",
              "2b8a045c77b7d5d34b29a5f3fceca83f6ed7ceffa06575deb8263dbbb49f8885",
              "31f331d147f8c9c614b609d4d72116a35944ee1be0fa9c9988ebdc7acff83489",
              "31f331d147f8c9c614b609d4d72116a35944ee1be0fa9c9988ebdc7acff83489",
              "925f630ef6853a726ea48fcc20d7c3c977060099813f2ebec433ebbcb104c8d5",
              "925f630ef6853a726ea48fcc20d7c3c977060099813f2ebec433ebbcb104c8d5",
              "1faa37dfdd4d8cf411f34686df5455422945d0fb170ae8683c972037e50a59a0",
              "1faa37dfdd4d8cf411f34686df5455422945d0fb170ae8683c972037e50a59a0"
            ],
            "raw_response_sha256": [
              "3464bc49b96a6dc5f859da51a91a0a96ee38faee51e871359f78186e12b73209",
              "3464bc49b96a6dc5f859da51a91a0a96ee38faee51e871359f78186e12b73209",
              "af3ea973ce4e3a4b405159ec49f20927cafda7a75c983dc1255769a42f06fafe",
              "1762fd8e50e7c5474e644d2893879770087b331e98180ae94038acfabceb2fb9",
              "4735f062ea70f0eb19956c51c8f3c657965d8ec1328d7372973f7a5531c4afa2",
              "4735f062ea70f0eb19956c51c8f3c657965d8ec1328d7372973f7a5531c4afa2",
              "c7fdd49c341af09baec94eb5c5a1dfa0d4d7e4d3262b93567c334877659a7c5d",
              "db56e5a658b7362c25eaa5e9c2b521eff1316f98627e4ee341aa2e9737f8bd4b",
              "8eaf395536a2e3ee66a7a289e73afa975e5f47c95320ac4fa3b192a8fe24156e",
              "99cc3fe81c17ef427ca83b56747e0f3b55092de875ccac7eaebad6c6475c0b25",
              "3325cd349e2989082e5093b68b6b8e506f7af80b93382d79eb4f89c42dd7c0bb",
              "99b031ad8baa0944171b9d906b5734bc454ff6e2ec0e64d5de87d92bdd39b4ac",
              "77b137a252bd3c6422913890f728bd43215e8e1eb2c8dab54181b46c7bc59a67",
              "77b137a252bd3c6422913890f728bd43215e8e1eb2c8dab54181b46c7bc59a67",
              "99c8b0360fd0bc2390dca952fe26358634e6a3b79d86070d54c89020871de0c3",
              "99c8b0360fd0bc2390dca952fe26358634e6a3b79d86070d54c89020871de0c3",
              "1d7eac2a65c87b054dfb1b07a81d6de7a5de9f8ce8decd78118b7d96c609a6f6",
              "b6185f492d66f2f66b255c5fbd9eade5f38d70c6a61fa057a97adf891f4de207",
              "e94848fe46597ddeb77b8eb8cf528584ae84496e987a3106912e8473b19cdc2b",
              "e94848fe46597ddeb77b8eb8cf528584ae84496e987a3106912e8473b19cdc2b",
              "b4bed04261b32969d8af85edec1db30fd2c698b46df747a8f87cfd64766e9791",
              "8da9d0568ec7fecd767747ed662f375a05a81af072d95c01a1f435b741ce2e32",
              "2f2c0534bb32258eaaee3f12117f0b2ee9c6361ebf24402514c4ba79bda3c88e",
              "2bcc7a2c12c1228f87605c6bc115161bd0fb90654fc3f025bb97d388254ee18d",
              "47b67ee03eb5f315a636937a0ad2e05021c4ae60912d4b700cc7108e36cb8f5c",
              "47b67ee03eb5f315a636937a0ad2e05021c4ae60912d4b700cc7108e36cb8f5c",
              "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
              "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
              "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
              "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
              "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
              "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
              "fbca7a32d7302cf8445f28461b72e4711e1cd4649921d876e3c92e1a7201563c",
              "fbca7a32d7302cf8445f28461b72e4711e1cd4649921d876e3c92e1a7201563c",
              "1e428274651d65945d123c6519e4f9d47210ecf2c61fb693000bb0f656db6267",
              "1e428274651d65945d123c6519e4f9d47210ecf2c61fb693000bb0f656db6267",
              "5588123a140ed7308bb5e383dd7c02ecc1f80b1f9582cf4569c63752615a6313",
              "a7225bc4d86ad4e2515b8d7151e6848efc245183124faf7922983533a1f57696",
              "40777264c33ba28f5d41b2a9522a914f75b7cdff641f80dc74c657c43ba109d2",
              "40777264c33ba28f5d41b2a9522a914f75b7cdff641f80dc74c657c43ba109d2",
              "38a6129ba7414f3d0cf0c341a94f0d8b8fe84eb14914b37fa5f34c150cf7a6ea",
              "d925bf9e23ab6410d9657829c08df0c4597adb7bcc3c63881385eb3cf094fa24",
              "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
              "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
              "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
              "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
              "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
              "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134"
            ],
            "run_ids": [
              "ANC-075-round3-default-fixture_pass-r1",
              "ANC-075-round3-default-fixture_pass-r2",
              "ANC-075-round3-default-fixture_fail-r1",
              "ANC-075-round3-default-fixture_fail-r2",
              "ANC-075-round3-default-paraphrase-r1",
              "ANC-075-round3-default-paraphrase-r2",
              "ANC-075-round3-default-silent-r1",
              "ANC-075-round3-default-silent-r2",
              "ANC-075-round3-default-topical-r1",
              "ANC-075-round3-default-topical-r2",
              "ANC-075-round3-default-conflict-r1",
              "ANC-075-round3-default-conflict-r2",
              "ANC-075-round3-default-irrelevant-r1",
              "ANC-075-round3-default-irrelevant-r2",
              "ANC-075-round3-default-boundary-r1",
              "ANC-075-round3-default-boundary-r2",
              "ANC-075-round3-specific-fixture_pass-r1",
              "ANC-075-round3-specific-fixture_pass-r2",
              "ANC-075-round3-specific-fixture_fail-r1",
              "ANC-075-round3-specific-fixture_fail-r2",
              "ANC-075-round3-specific-paraphrase-r1",
              "ANC-075-round3-specific-paraphrase-r2",
              "ANC-075-round3-specific-silent-r1",
              "ANC-075-round3-specific-silent-r2",
              "ANC-075-round3-specific-topical-r1",
              "ANC-075-round3-specific-topical-r2",
              "ANC-075-round3-specific-conflict-r1",
              "ANC-075-round3-specific-conflict-r2",
              "ANC-075-round3-specific-irrelevant-r1",
              "ANC-075-round3-specific-irrelevant-r2",
              "ANC-075-round3-specific-boundary-r1",
              "ANC-075-round3-specific-boundary-r2",
              "ANC-075-round3-output-fixture_pass-r1",
              "ANC-075-round3-output-fixture_pass-r2",
              "ANC-075-round3-output-fixture_fail-r1",
              "ANC-075-round3-output-fixture_fail-r2",
              "ANC-075-round3-output-paraphrase-r1",
              "ANC-075-round3-output-paraphrase-r2",
              "ANC-075-round3-output-silent-r1",
              "ANC-075-round3-output-silent-r2",
              "ANC-075-round3-output-topical-r1",
              "ANC-075-round3-output-topical-r2",
              "ANC-075-round3-output-conflict-r1",
              "ANC-075-round3-output-conflict-r2",
              "ANC-075-round3-output-irrelevant-r1",
              "ANC-075-round3-output-irrelevant-r2",
              "ANC-075-round3-output-boundary-r1",
              "ANC-075-round3-output-boundary-r2"
            ]
          },
          "score": {
            "exact_role_cells": 48,
            "total_role_cells": 48,
            "exact_semantic_verdicts": 16,
            "total_semantic_verdicts": 16,
            "stable_cases": 8,
            "total_cases": 8,
            "regressions": 0,
            "confidence": {
              "threshold": 0.8,
              "cells_at_or_above_threshold": 46,
              "total_cells": 48,
              "minimum": 0.64,
              "p10": 0.97,
              "median": 1.0,
              "mean": 0.98
            }
          },
          "decision": {
            "status": "sealed",
            "question_set_sha256": "1aa0372012b96a05b252004cc38c88e207bbddf83df3029e0faf3678663e46eb",
            "reason_code": "TOURNAMENT.EXACT"
          },
          "supersedes": null
        }
      ]
    },
    "tuning_history_after_sha256": "214697e9f388ded83c67324cfcba907999677e3fd9b0067a0d4781ed0d80bcbb",
    "production_semantics_unchanged": true,
    "authority_citations": [
      "ANC-075 v2 locked quote from atm-bd-orchestration issue #349",
      "ANC-075 v2 approved pass/fail fixtures"
    ],
    "oracle_sha256": "8fa19a1aa07ef2bb4cd8bf97bd56ae2b0bc90913ea3dda01647529d6456be518",
    "sealed_question_set": {
      "digest": "1aa0372012b96a05b252004cc38c88e207bbddf83df3029e0faf3678663e46eb",
      "questions": {
        "relevant": {
          "type": "choice",
          "instructions": "Compare `item.text` with `anchor.scope`. Does the item operate on package publishing, release contents, shipped file selection, or the archive manifest?",
          "criteria": {
            "yes": "It works on the skill package publisher or its release artifact and files.",
            "no": "It works wholly outside the package publishing and archive area.",
            "unclear": "The affected package or output cannot be identified."
          }
        },
        "addressed": {
          "type": "choice",
          "instructions": "Read `item.text` for an affirmative output contract matching `anchor.statement`: one sc-sha path/hash entry for each file actually shipped among agents, skills, Python, templates, and scripts, with an aggregate in the archive. Do not infer this from a heading.",
          "criteria": {
            "yes": "The publisher is planned to emit the complete per-file sc-sha entries and aggregate in the release archive.",
            "no": "No affirmative complete output contract is stated, even if a manifest is named.",
            "unclear": "The output commitment is stated but coverage of shipped files or aggregate cannot be determined."
          }
        },
        "contradicted": {
          "type": "choice",
          "instructions": "Would an explicit decision in `item.text` prevent the output required by `anchor.statement`? Distinguish an explicit exclusion from mere absence of detail.",
          "criteria": {
            "yes": "The text expressly limits the files included, rejects sc-sha, defers the aggregate, or leaves the manifest out of the artifact.",
            "no": "No stated decision rules out the full output; silence is no.",
            "unclear": "The text suggests a possible exclusion but does not settle it."
          }
        }
      }
    },
    "candidate_rankings": [
      {
        "candidate_id": "default",
        "question_sha256": "8e9e29b043185ff7a76912e1d28726222bcd3fc867dcabb78a9bef523029ec05",
        "score": {
          "exact_role_cells": 44,
          "total_role_cells": 48,
          "exact_semantic_verdicts": 14,
          "total_semantic_verdicts": 16,
          "stable_cases": 8,
          "total_cases": 8,
          "regressions": 0
        },
        "confidence": {
          "threshold": 0.8,
          "cells_at_or_above_threshold": 40,
          "total_cells": 48,
          "minimum": 0.58,
          "p10": 0.7,
          "median": 1.0,
          "mean": 0.9291666666666667
        },
        "accepted": false,
        "complexity": 2542
      },
      {
        "candidate_id": "specific",
        "question_sha256": "c8ece1cb879a50f2f3c70366f37c1036814895a6a07df73b47c767c7bee6eebc",
        "score": {
          "exact_role_cells": 48,
          "total_role_cells": 48,
          "exact_semantic_verdicts": 16,
          "total_semantic_verdicts": 16,
          "stable_cases": 8,
          "total_cases": 8,
          "regressions": 0
        },
        "confidence": {
          "threshold": 0.8,
          "cells_at_or_above_threshold": 43,
          "total_cells": 48,
          "minimum": 0.43,
          "p10": 0.73,
          "median": 1.0,
          "mean": 0.936875
        },
        "accepted": true,
        "complexity": 1588
      },
      {
        "candidate_id": "output",
        "question_sha256": "1aa0372012b96a05b252004cc38c88e207bbddf83df3029e0faf3678663e46eb",
        "score": {
          "exact_role_cells": 48,
          "total_role_cells": 48,
          "exact_semantic_verdicts": 16,
          "total_semantic_verdicts": 16,
          "stable_cases": 8,
          "total_cases": 8,
          "regressions": 0
        },
        "confidence": {
          "threshold": 0.8,
          "cells_at_or_above_threshold": 46,
          "total_cells": 48,
          "minimum": 0.64,
          "p10": 0.97,
          "median": 1.0,
          "mean": 0.98
        },
        "accepted": true,
        "complexity": 1552
      }
    ],
    "acceptance_table": [
      {
        "candidate_id": "default",
        "accepted": false,
        "score": {
          "exact_role_cells": 44,
          "total_role_cells": 48,
          "exact_semantic_verdicts": 14,
          "total_semantic_verdicts": 16,
          "stable_cases": 8,
          "total_cases": 8,
          "regressions": 0
        },
        "confidence": {
          "threshold": 0.8,
          "cells_at_or_above_threshold": 40,
          "total_cells": 48,
          "minimum": 0.58,
          "p10": 0.7,
          "median": 1.0,
          "mean": 0.9291666666666667
        }
      },
      {
        "candidate_id": "specific",
        "accepted": true,
        "score": {
          "exact_role_cells": 48,
          "total_role_cells": 48,
          "exact_semantic_verdicts": 16,
          "total_semantic_verdicts": 16,
          "stable_cases": 8,
          "total_cases": 8,
          "regressions": 0
        },
        "confidence": {
          "threshold": 0.8,
          "cells_at_or_above_threshold": 43,
          "total_cells": 48,
          "minimum": 0.43,
          "p10": 0.73,
          "median": 1.0,
          "mean": 0.936875
        }
      },
      {
        "candidate_id": "output",
        "accepted": true,
        "score": {
          "exact_role_cells": 48,
          "total_role_cells": 48,
          "exact_semantic_verdicts": 16,
          "total_semantic_verdicts": 16,
          "stable_cases": 8,
          "total_cases": 8,
          "regressions": 0
        },
        "confidence": {
          "threshold": 0.8,
          "cells_at_or_above_threshold": 46,
          "total_cells": 48,
          "minimum": 0.64,
          "p10": 0.97,
          "median": 1.0,
          "mean": 0.98
        }
      }
    ],
    "evidence": {
      "oracle": [
        {
          "id": "fixture_pass",
          "text": "The package publish script puts path/hash manifest entries for every shipped agent, skill, Python file, template, and script in the release artifact and includes the aggregate.",
          "expected": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "verdict": "satisfied",
          "rationale": "Approved pass fixture explicitly includes every shipped category, per-file hashes, and aggregate."
        },
        {
          "id": "fixture_fail",
          "text": "The package publish script includes only Python files in the release manifest and omits the aggregate hash.",
          "expected": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "verdict": "contradicted",
          "rationale": "Approved fail fixture explicitly restricts coverage and omits aggregate."
        },
        {
          "id": "paraphrase",
          "text": "The skill package publisher lists each released agent, skill, Python file, template, and script with its sc-sha in the archive manifest and records their aggregate.",
          "expected": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "verdict": "satisfied",
          "rationale": "Equivalent complete obligation."
        },
        {
          "id": "silent",
          "text": "Change the package publish script to stamp the release date in archive metadata.",
          "expected": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "verdict": "not_addressed",
          "rationale": "Publisher touched with no manifest directive."
        },
        {
          "id": "topical",
          "text": "Add a hash manifest to the skill release artifact.",
          "expected": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "verdict": "not_addressed",
          "rationale": "Generic topic leaves the required output unspecified."
        },
        {
          "id": "conflict",
          "text": "Publish a manifest for Python files only, and omit agents, skills, templates, scripts, and the aggregate.",
          "expected": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "verdict": "contradicted",
          "rationale": "Explicitly excludes required output."
        },
        {
          "id": "irrelevant",
          "text": "Change the task-close hook to retry sanity assignment after a network failure.",
          "expected": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "verdict": "not_applicable",
          "rationale": "Different system activity."
        },
        {
          "id": "boundary",
          "text": "Have the package publish script list every shipped agent, skill, Python file, template, and script with per-file sc-sha, but leave the aggregate for a later release.",
          "expected": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "verdict": "contradicted",
          "rationale": "Explicitly defers required aggregate."
        }
      ],
      "candidate_responses": [
        {
          "candidate_id": "default",
          "case_id": "fixture_pass",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-fixture_pass-r1",
          "canonical_request_sha256": "1eeb797fdceae45de9e36ec04bb6adfc933ddd33e2cac0f4dd3da4234faf6407",
          "raw_response_sha256": "3464bc49b96a6dc5f859da51a91a0a96ee38faee51e871359f78186e12b73209",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.98,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "fixture_pass",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-fixture_pass-r2",
          "canonical_request_sha256": "1eeb797fdceae45de9e36ec04bb6adfc933ddd33e2cac0f4dd3da4234faf6407",
          "raw_response_sha256": "3464bc49b96a6dc5f859da51a91a0a96ee38faee51e871359f78186e12b73209",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.98,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "fixture_fail",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-fixture_fail-r1",
          "canonical_request_sha256": "17987fa8712e01b4c6cda09d941421baa277694b80eee9927677b6899f4384cc",
          "raw_response_sha256": "af3ea973ce4e3a4b405159ec49f20927cafda7a75c983dc1255769a42f06fafe",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.58,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "fixture_fail",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-fixture_fail-r2",
          "canonical_request_sha256": "17987fa8712e01b4c6cda09d941421baa277694b80eee9927677b6899f4384cc",
          "raw_response_sha256": "1762fd8e50e7c5474e644d2893879770087b331e98180ae94038acfabceb2fb9",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.61,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "paraphrase",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-paraphrase-r1",
          "canonical_request_sha256": "f9166037f166e451bf023db1ad140a22433d29fd5d69229cca784e82c7460ece",
          "raw_response_sha256": "4735f062ea70f0eb19956c51c8f3c657965d8ec1328d7372973f7a5531c4afa2",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "paraphrase",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-paraphrase-r2",
          "canonical_request_sha256": "f9166037f166e451bf023db1ad140a22433d29fd5d69229cca784e82c7460ece",
          "raw_response_sha256": "4735f062ea70f0eb19956c51c8f3c657965d8ec1328d7372973f7a5531c4afa2",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "silent",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-silent-r1",
          "canonical_request_sha256": "48adbb87a784c1662eb7db0a78a8c63b9c82ee284b4a1a4036f0efbd5dd9e181",
          "raw_response_sha256": "c7fdd49c341af09baec94eb5c5a1dfa0d4d7e4d3262b93567c334877659a7c5d",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.73,
            "compliance": 0.98,
            "conflict": 0.92
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "silent",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-silent-r2",
          "canonical_request_sha256": "48adbb87a784c1662eb7db0a78a8c63b9c82ee284b4a1a4036f0efbd5dd9e181",
          "raw_response_sha256": "db56e5a658b7362c25eaa5e9c2b521eff1316f98627e4ee341aa2e9737f8bd4b",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.72,
            "compliance": 0.97,
            "conflict": 0.91
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "topical",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-topical-r1",
          "canonical_request_sha256": "af14df790d139b95c4a75e931fdd12a446b437105198df318d74709f022410b0",
          "raw_response_sha256": "8eaf395536a2e3ee66a7a289e73afa975e5f47c95320ac4fa3b192a8fe24156e",
          "roles": {
            "relevance": "yes",
            "compliance": "unclear",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.67,
            "conflict": 0.94
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "topical",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-topical-r2",
          "canonical_request_sha256": "af14df790d139b95c4a75e931fdd12a446b437105198df318d74709f022410b0",
          "raw_response_sha256": "99cc3fe81c17ef427ca83b56747e0f3b55092de875ccac7eaebad6c6475c0b25",
          "roles": {
            "relevance": "yes",
            "compliance": "unclear",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.7,
            "conflict": 0.89
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "conflict",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-conflict-r1",
          "canonical_request_sha256": "edf8aaf8c631a16fdca37770dc49dcdc2a31a9a650eddb2155b38552ce3938fd",
          "raw_response_sha256": "3325cd349e2989082e5093b68b6b8e506f7af80b93382d79eb4f89c42dd7c0bb",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.73,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "conflict",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-conflict-r2",
          "canonical_request_sha256": "edf8aaf8c631a16fdca37770dc49dcdc2a31a9a650eddb2155b38552ce3938fd",
          "raw_response_sha256": "99b031ad8baa0944171b9d906b5734bc454ff6e2ec0e64d5de87d92bdd39b4ac",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.69,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "irrelevant",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-irrelevant-r1",
          "canonical_request_sha256": "766e77857cf0693ed90c457cfc58d7816f522946a955f3e30f324d722f1fded8",
          "raw_response_sha256": "77b137a252bd3c6422913890f728bd43215e8e1eb2c8dab54181b46c7bc59a67",
          "roles": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "irrelevant",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-irrelevant-r2",
          "canonical_request_sha256": "766e77857cf0693ed90c457cfc58d7816f522946a955f3e30f324d722f1fded8",
          "raw_response_sha256": "77b137a252bd3c6422913890f728bd43215e8e1eb2c8dab54181b46c7bc59a67",
          "roles": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "boundary",
          "repeat": 1,
          "run_id": "ANC-075-round3-default-boundary-r1",
          "canonical_request_sha256": "2a58e6252bd4c350f3685e67d35b1f77f520bd282861fd33a2d48a6093032186",
          "raw_response_sha256": "99c8b0360fd0bc2390dca952fe26358634e6a3b79d86070d54c89020871de0c3",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.8,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "default",
          "case_id": "boundary",
          "repeat": 2,
          "run_id": "ANC-075-round3-default-boundary-r2",
          "canonical_request_sha256": "2a58e6252bd4c350f3685e67d35b1f77f520bd282861fd33a2d48a6093032186",
          "raw_response_sha256": "99c8b0360fd0bc2390dca952fe26358634e6a3b79d86070d54c89020871de0c3",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.8,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "fixture_pass",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-fixture_pass-r1",
          "canonical_request_sha256": "f054a4c0020eacde75feeabe0f4163ea73fa714b95c90429e5fe2fa0ee5cdb4b",
          "raw_response_sha256": "1d7eac2a65c87b054dfb1b07a81d6de7a5de9f8ce8decd78118b7d96c609a6f6",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.8,
            "compliance": 0.46,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "fixture_pass",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-fixture_pass-r2",
          "canonical_request_sha256": "f054a4c0020eacde75feeabe0f4163ea73fa714b95c90429e5fe2fa0ee5cdb4b",
          "raw_response_sha256": "b6185f492d66f2f66b255c5fbd9eade5f38d70c6a61fa057a97adf891f4de207",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.73,
            "compliance": 0.43,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "fixture_fail",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-fixture_fail-r1",
          "canonical_request_sha256": "e4c9f036721c3020cada51c525affa8be4a71651ff6e26b7fde39bd56a766f7b",
          "raw_response_sha256": "e94848fe46597ddeb77b8eb8cf528584ae84496e987a3106912e8473b19cdc2b",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.98,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "fixture_fail",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-fixture_fail-r2",
          "canonical_request_sha256": "e4c9f036721c3020cada51c525affa8be4a71651ff6e26b7fde39bd56a766f7b",
          "raw_response_sha256": "e94848fe46597ddeb77b8eb8cf528584ae84496e987a3106912e8473b19cdc2b",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.98,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "paraphrase",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-paraphrase-r1",
          "canonical_request_sha256": "cb4be84a8116946a60d5f5d423b8ab1722428ffd8f95cb8216ab07dd0d7eb82a",
          "raw_response_sha256": "b4bed04261b32969d8af85edec1db30fd2c698b46df747a8f87cfd64766e9791",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.56,
            "compliance": 0.89,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "paraphrase",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-paraphrase-r2",
          "canonical_request_sha256": "cb4be84a8116946a60d5f5d423b8ab1722428ffd8f95cb8216ab07dd0d7eb82a",
          "raw_response_sha256": "8da9d0568ec7fecd767747ed662f375a05a81af072d95c01a1f435b741ce2e32",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.61,
            "compliance": 0.88,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "silent",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-silent-r1",
          "canonical_request_sha256": "0dc08a52527467a3dff70070bd03deee68f72ca57a0b8fafafcaab0761b953cf",
          "raw_response_sha256": "2f2c0534bb32258eaaee3f12117f0b2ee9c6361ebf24402514c4ba79bda3c88e",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.94,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "silent",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-silent-r2",
          "canonical_request_sha256": "0dc08a52527467a3dff70070bd03deee68f72ca57a0b8fafafcaab0761b953cf",
          "raw_response_sha256": "2bcc7a2c12c1228f87605c6bc115161bd0fb90654fc3f025bb97d388254ee18d",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.95,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "topical",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-topical-r1",
          "canonical_request_sha256": "eaba2b2b3a78ca7ed4487aa045d54e4618d539b4ee6ebd8cda5f55775bec3c6a",
          "raw_response_sha256": "47b67ee03eb5f315a636937a0ad2e05021c4ae60912d4b700cc7108e36cb8f5c",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.95,
            "conflict": 0.95
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "topical",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-topical-r2",
          "canonical_request_sha256": "eaba2b2b3a78ca7ed4487aa045d54e4618d539b4ee6ebd8cda5f55775bec3c6a",
          "raw_response_sha256": "47b67ee03eb5f315a636937a0ad2e05021c4ae60912d4b700cc7108e36cb8f5c",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.95,
            "conflict": 0.95
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "conflict",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-conflict-r1",
          "canonical_request_sha256": "a125cb40df8a558f9076b51459e71134b2d9f04d68e7437495bf5920df2d21d5",
          "raw_response_sha256": "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "conflict",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-conflict-r2",
          "canonical_request_sha256": "a125cb40df8a558f9076b51459e71134b2d9f04d68e7437495bf5920df2d21d5",
          "raw_response_sha256": "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "irrelevant",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-irrelevant-r1",
          "canonical_request_sha256": "46b00754419d2f1ad7ff08b5502be4bacfc23fb8df5b04974b814a076069cb50",
          "raw_response_sha256": "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
          "roles": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "irrelevant",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-irrelevant-r2",
          "canonical_request_sha256": "46b00754419d2f1ad7ff08b5502be4bacfc23fb8df5b04974b814a076069cb50",
          "raw_response_sha256": "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
          "roles": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "boundary",
          "repeat": 1,
          "run_id": "ANC-075-round3-specific-boundary-r1",
          "canonical_request_sha256": "67d543dd73c21717b91e32cf8f2efcc57a7cea3d39d4f8c17117ccf981f07cf2",
          "raw_response_sha256": "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 0.98
          },
          "error": null
        },
        {
          "candidate_id": "specific",
          "case_id": "boundary",
          "repeat": 2,
          "run_id": "ANC-075-round3-specific-boundary-r2",
          "canonical_request_sha256": "67d543dd73c21717b91e32cf8f2efcc57a7cea3d39d4f8c17117ccf981f07cf2",
          "raw_response_sha256": "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 0.98
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "fixture_pass",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-fixture_pass-r1",
          "canonical_request_sha256": "b6b7984d31225ea6cd2c9f8ecbd54c66b9d071d93c3436ab47b419d8f4bfc362",
          "raw_response_sha256": "fbca7a32d7302cf8445f28461b72e4711e1cd4649921d876e3c92e1a7201563c",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.97,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "fixture_pass",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-fixture_pass-r2",
          "canonical_request_sha256": "b6b7984d31225ea6cd2c9f8ecbd54c66b9d071d93c3436ab47b419d8f4bfc362",
          "raw_response_sha256": "fbca7a32d7302cf8445f28461b72e4711e1cd4649921d876e3c92e1a7201563c",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.97,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "fixture_fail",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-fixture_fail-r1",
          "canonical_request_sha256": "2f4c1d9f93f75290be1fb6002cd8ff70e71c82fc98c325e15bde62b349016cc3",
          "raw_response_sha256": "1e428274651d65945d123c6519e4f9d47210ecf2c61fb693000bb0f656db6267",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "fixture_fail",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-fixture_fail-r2",
          "canonical_request_sha256": "2f4c1d9f93f75290be1fb6002cd8ff70e71c82fc98c325e15bde62b349016cc3",
          "raw_response_sha256": "1e428274651d65945d123c6519e4f9d47210ecf2c61fb693000bb0f656db6267",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "paraphrase",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-paraphrase-r1",
          "canonical_request_sha256": "9c1134ca53251e673b1ce1888d83c7fe04561e5d408c60b64296c2381fb1896c",
          "raw_response_sha256": "5588123a140ed7308bb5e383dd7c02ecc1f80b1f9582cf4569c63752615a6313",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.95,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "paraphrase",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-paraphrase-r2",
          "canonical_request_sha256": "9c1134ca53251e673b1ce1888d83c7fe04561e5d408c60b64296c2381fb1896c",
          "raw_response_sha256": "a7225bc4d86ad4e2515b8d7151e6848efc245183124faf7922983533a1f57696",
          "roles": {
            "relevance": "yes",
            "compliance": "yes",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.97,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "silent",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-silent-r1",
          "canonical_request_sha256": "ec2b67622fb3eecdbc5f5c9fbdd62d412dbe519dd8758bca2f6e67a26fee84fe",
          "raw_response_sha256": "40777264c33ba28f5d41b2a9522a914f75b7cdff641f80dc74c657c43ba109d2",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.98,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "silent",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-silent-r2",
          "canonical_request_sha256": "ec2b67622fb3eecdbc5f5c9fbdd62d412dbe519dd8758bca2f6e67a26fee84fe",
          "raw_response_sha256": "40777264c33ba28f5d41b2a9522a914f75b7cdff641f80dc74c657c43ba109d2",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 0.98,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "topical",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-topical-r1",
          "canonical_request_sha256": "2b8a045c77b7d5d34b29a5f3fceca83f6ed7ceffa06575deb8263dbbb49f8885",
          "raw_response_sha256": "38a6129ba7414f3d0cf0c341a94f0d8b8fe84eb14914b37fa5f34c150cf7a6ea",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.64,
            "conflict": 0.97
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "topical",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-topical-r2",
          "canonical_request_sha256": "2b8a045c77b7d5d34b29a5f3fceca83f6ed7ceffa06575deb8263dbbb49f8885",
          "raw_response_sha256": "d925bf9e23ab6410d9657829c08df0c4597adb7bcc3c63881385eb3cf094fa24",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 0.67,
            "conflict": 0.98
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "conflict",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-conflict-r1",
          "canonical_request_sha256": "31f331d147f8c9c614b609d4d72116a35944ee1be0fa9c9988ebdc7acff83489",
          "raw_response_sha256": "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "conflict",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-conflict-r2",
          "canonical_request_sha256": "31f331d147f8c9c614b609d4d72116a35944ee1be0fa9c9988ebdc7acff83489",
          "raw_response_sha256": "68ed68b8925cc9f67be13e757e1bd262a15c3a087f77519b157b44fc14e8fabc",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "irrelevant",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-irrelevant-r1",
          "canonical_request_sha256": "925f630ef6853a726ea48fcc20d7c3c977060099813f2ebec433ebbcb104c8d5",
          "raw_response_sha256": "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
          "roles": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "irrelevant",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-irrelevant-r2",
          "canonical_request_sha256": "925f630ef6853a726ea48fcc20d7c3c977060099813f2ebec433ebbcb104c8d5",
          "raw_response_sha256": "240c4bd51ddd79b2bce60218a404e41f53229aafe07c2652b6e4ec5e8737481e",
          "roles": {
            "relevance": "no",
            "compliance": "no",
            "conflict": "no"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 1.0
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "boundary",
          "repeat": 1,
          "run_id": "ANC-075-round3-output-boundary-r1",
          "canonical_request_sha256": "1faa37dfdd4d8cf411f34686df5455422945d0fb170ae8683c972037e50a59a0",
          "raw_response_sha256": "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 0.98
          },
          "error": null
        },
        {
          "candidate_id": "output",
          "case_id": "boundary",
          "repeat": 2,
          "run_id": "ANC-075-round3-output-boundary-r2",
          "canonical_request_sha256": "1faa37dfdd4d8cf411f34686df5455422945d0fb170ae8683c972037e50a59a0",
          "raw_response_sha256": "dfe604f083299990e1c4757f88ed001a0ae5e310ef32d120c99f9284c81f3134",
          "roles": {
            "relevance": "yes",
            "compliance": "no",
            "conflict": "yes"
          },
          "confidence": {
            "relevance": 1.0,
            "compliance": 1.0,
            "conflict": 0.98
          },
          "error": null
        }
      ]
    },
    "explanation": "The oracle was frozen before Jev. Three role-complete candidates each ran twice on every case. Exact role labels, threshold-zero verdicts, repeat stability, and zero regressions determine semantic acceptance.",
    "planner_action": "Review the frozen oracle and acceptance table before installing the sealed override.",
    "question_action": null,
    "sealed_question_set_digest_before": null,
    "sealed_question_set_digest_after": "1aa0372012b96a05b252004cc38c88e207bbddf83df3029e0faf3678663e46eb",
    "authority_recommendations": []
  },
  "error": null,
  "metadata": {
    "duration_ms": 1499,
    "tool_calls": 48,
    "retry_count": 0,
    "correlation_id": "ANC-075",
    "jev_run_ids": [
      "ANC-075-round3-default-fixture_pass-r1",
      "ANC-075-round3-default-fixture_pass-r2",
      "ANC-075-round3-default-fixture_fail-r1",
      "ANC-075-round3-default-fixture_fail-r2",
      "ANC-075-round3-default-paraphrase-r1",
      "ANC-075-round3-default-paraphrase-r2",
      "ANC-075-round3-default-silent-r1",
      "ANC-075-round3-default-silent-r2",
      "ANC-075-round3-default-topical-r1",
      "ANC-075-round3-default-topical-r2",
      "ANC-075-round3-default-conflict-r1",
      "ANC-075-round3-default-conflict-r2",
      "ANC-075-round3-default-irrelevant-r1",
      "ANC-075-round3-default-irrelevant-r2",
      "ANC-075-round3-default-boundary-r1",
      "ANC-075-round3-default-boundary-r2",
      "ANC-075-round3-specific-fixture_pass-r1",
      "ANC-075-round3-specific-fixture_pass-r2",
      "ANC-075-round3-specific-fixture_fail-r1",
      "ANC-075-round3-specific-fixture_fail-r2",
      "ANC-075-round3-specific-paraphrase-r1",
      "ANC-075-round3-specific-paraphrase-r2",
      "ANC-075-round3-specific-silent-r1",
      "ANC-075-round3-specific-silent-r2",
      "ANC-075-round3-specific-topical-r1",
      "ANC-075-round3-specific-topical-r2",
      "ANC-075-round3-specific-conflict-r1",
      "ANC-075-round3-specific-conflict-r2",
      "ANC-075-round3-specific-irrelevant-r1",
      "ANC-075-round3-specific-irrelevant-r2",
      "ANC-075-round3-specific-boundary-r1",
      "ANC-075-round3-specific-boundary-r2",
      "ANC-075-round3-output-fixture_pass-r1",
      "ANC-075-round3-output-fixture_pass-r2",
      "ANC-075-round3-output-fixture_fail-r1",
      "ANC-075-round3-output-fixture_fail-r2",
      "ANC-075-round3-output-paraphrase-r1",
      "ANC-075-round3-output-paraphrase-r2",
      "ANC-075-round3-output-silent-r1",
      "ANC-075-round3-output-silent-r2",
      "ANC-075-round3-output-topical-r1",
      "ANC-075-round3-output-topical-r2",
      "ANC-075-round3-output-conflict-r1",
      "ANC-075-round3-output-conflict-r2",
      "ANC-075-round3-output-irrelevant-r1",
      "ANC-075-round3-output-irrelevant-r2",
      "ANC-075-round3-output-boundary-r1",
      "ANC-075-round3-output-boundary-r2"
    ],
    "planner_observation_trust": null,
    "authoritative_sources_unchanged": true
  }
}
```
