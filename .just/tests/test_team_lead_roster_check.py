"""Unit tests for the team-lead skill's read-only roster check."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import tomllib
import unittest

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / ".claude/skills/team-lead/scripts/roster_check.py"
spec = importlib.util.spec_from_file_location("roster_check", SCRIPT)
roster_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(roster_check)

CONFIG = {"team-lead": "atm-lead", "quality-mgr": "atm-quality", "publisher": "atm-publisher", "arch-ctm": None}
ROSTER = [
    {"name": "team-lead", "alias": "atm-lead", "backend": "herdr"},
    {"name": "quality-mgr", "alias": "atm-quality", "backend": "herdr"},
    {"name": "publisher", "alias": "atm-publisher", "backend": "herdr"},
    {"name": "arch-ctm", "alias": None, "backend": "herdr"},
]
HERDR = ["atm-lead", "atm-quality", "atm-publisher", "arch-ctm", "obs-lead"]


def edited(name: str, **changes: object) -> list[dict]:
    return [{**member, **changes} if member["name"] == name else member for member in ROSTER]


class FindProblemsTests(unittest.TestCase):
    def test_agreeing_sources_have_no_problems(self) -> None:
        self.assertEqual(roster_check.find_problems(CONFIG, ROSTER, HERDR), [])

    def test_dropped_roster_alias_is_reported_twice(self) -> None:
        problems = roster_check.find_problems(CONFIG, edited("publisher", alias=None), HERDR)
        self.assertIn("publisher: roster alias missing (required for a unique Herdr name)", problems)
        self.assertIn("publisher: .atm.toml alias 'atm-publisher' != roster alias None", problems)

    def test_required_alias_missing_from_config(self) -> None:
        problems = roster_check.find_problems({**CONFIG, "team-lead": None}, ROSTER, HERDR)
        self.assertIn("team-lead: .atm.toml declares no alias (required)", problems)

    def test_missing_herdr_agent(self) -> None:
        problems = roster_check.find_problems(CONFIG, ROSTER, [n for n in HERDR if n != "arch-ctm"])
        self.assertEqual(problems, ["arch-ctm: no live Herdr agent named 'arch-ctm'"])

    def test_bare_name_on_another_team_does_not_satisfy_an_aliased_member(self) -> None:
        herdr = [n for n in HERDR if n != "atm-publisher"] + ["publisher"]
        problems = roster_check.find_problems(CONFIG, ROSTER, herdr)
        self.assertEqual(problems, ["publisher: no live Herdr agent named 'atm-publisher'"])

    def test_duplicate_herdr_name(self) -> None:
        problems = roster_check.find_problems(CONFIG, ROSTER, HERDR + ["arch-ctm"])
        self.assertEqual(problems, ["Herdr agent name 'arch-ctm' is not unique on this session"])

    def test_roster_and_config_membership_must_match(self) -> None:
        roster = ROSTER + [{"name": "solar", "alias": None, "backend": "herdr"}]
        problems = roster_check.find_problems({**CONFIG, "spare-dev": None}, roster, HERDR + ["solar"])
        self.assertEqual(
            problems,
            ["spare-dev: declared in .atm.toml but not in the roster",
             "solar: in the roster but has no pane in .atm.toml"],
        )


class ConfigAliasesTests(unittest.TestCase):
    def test_reads_only_this_teams_identified_panes(self) -> None:
        config = tomllib.loads(
            '[[rmux.windows]]\n'
            '[[rmux.windows.panes]]\nname = "team-lead"\nalias = "atm-lead"\n'
            'env = { ATM_IDENTITY = "team-lead", ATM_TEAM = "atm-dev" }\n'
            '[[rmux.windows.panes]]\nname = "arch-ctm"\n'
            'env = { ATM_IDENTITY = "arch-ctm", ATM_TEAM = "atm-dev" }\n'
            '[[rmux.windows.panes]]\nname = "spare"\nenv = { ATM_TEAM = "atm-dev" }\n'
            '[[rmux.windows.panes]]\nname = "other"\nalias = "x"\n'
            'env = { ATM_IDENTITY = "other", ATM_TEAM = "sc-obs" }\n'
        )
        self.assertEqual(
            roster_check.config_aliases(config, "atm-dev"),
            {"team-lead": "atm-lead", "arch-ctm": None},
        )

    def test_repo_atm_toml_declares_every_required_alias(self) -> None:
        config = tomllib.loads((REPO_ROOT / ".atm.toml").read_text("utf-8"))
        aliases = roster_check.config_aliases(config, "atm-dev")
        for name in roster_check.ALIAS_REQUIRED:
            with self.subTest(member=name):
                self.assertTrue(aliases.get(name))
        values = [alias for alias in aliases.values() if alias]
        self.assertEqual(len(values), len(set(values)))


if __name__ == "__main__":
    unittest.main()
