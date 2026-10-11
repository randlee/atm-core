"""Unit tests for scripts/jev_judge.py. Standard library only; no network is ever touched.

The Jev transport (`jev_judge.call_jev`) is replaced with a stub, so these tests prove the deterministic
half of the skill (IDs, gating, policy, coverage, diffs, reports), not Jev's own behaviour.
"""
import contextlib
import datetime as _dt
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import jev_judge as jj  # noqa: E402

PLAN_ROLES = {"relevance": "relevant", "compliance": "addressed", "conflict": "contradicted"}
DEV_ROLES = {"relevance": "relevant", "compliance": "implemented", "conflict": "violated", "evidence": "evidenced"}
OPTIONS = ("yes", "no", "unclear")
STATEMENT = "The runtime crates are created by copying the legacy repo's runtime crates and deleting the legacy-specific modules; no runtime crate is written from scratch."
QUOTE = "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code"
REQ_STATEMENT = "All public CLI commands must support --json output with a stable schema."
LIVE_ENV = {"TYPESAFE_API_KEY": "sk-test-0001", "PLAN_ANCHOR_ALLOW_REMOTE_JUDGE": "1"}
ASSETS = Path(__file__).resolve().parents[1] / "assets"


def answer(choice, p=0.9, options=OPTIONS):
    rest = round((1 - p) / (len(options) - 1), 4)
    probs = {o: (p if o == choice else rest) for o in options}
    return {"type": "choice", "choice": choice, "confidence": p, "probabilities": probs}


def plan_answers(rel="yes", comp="yes", conf="no", p=0.9, rel_p=None, comp_p=None, conf_p=None):
    return {"relevant": answer(rel, rel_p if rel_p is not None else p),
            "addressed": answer(comp, comp_p if comp_p is not None else p),
            "contradicted": answer(conf, conf_p if conf_p is not None else p)}


def dev_answers(rel="yes", comp="yes", conf="no", evid="yes", p=0.9, evid_p=None):
    return {"relevant": answer(rel, p), "implemented": answer(comp, p), "violated": answer(conf, p),
            "evidenced": answer(evid, evid_p if evid_p is not None else p)}


def anchor(aid="ANC-001", status="locked", source_type="ANC", source_ref=None, statement=STATEMENT, quote=None, **extra):
    """A valid anchor record. ANC anchors get the verbatim QUOTE; REQ/ADR anchors quote their register text."""
    if quote is None:
        quote = QUOTE if source_type == "ANC" else statement
    rec = {
        "id": aid, "source_type": source_type, "source_ref": source_ref, "statement": statement, "quote": quote,
        "stated_by": "the user", "stated_at": "2026-10-09", "context": "planning session",
        "summary": "start from the legacy repo", "scope": "the runtime crates", "deterministic_check": "",
        "fixtures": {"pass": [], "fail": []}, "question_overrides": {}, "version": 1, "status": status,
        "promoted_to": None, "beads": {"anchor": None, "plan_questions": None, "dev_questions": None}, "history": [],
    }
    rec.update(extra)
    return rec


def record(*anchors):
    return {"schema": "plan-anchoring/anchors/1", "plan": "phase-current", "intent_ref": "doc/plan/phase-current/intent.md",
            "anchors": list(anchors)}


def write(d, name, value):
    path = Path(d) / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value) if not isinstance(value, str) else value, encoding="utf-8")
    return str(path)


def run_cli(argv):
    """Run main() in-process; returns (exit code, parsed JSON or raw text)."""
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        code = jj.main(argv)
    out = buf.getvalue()
    try:
        return code, json.loads(out)
    except json.JSONDecodeError:
        return code, out


def env(**values):
    return mock.patch.dict(os.environ, values, clear=True)


class Transport:
    """Stub for jev_judge.call_jev: a queue of responses (dicts) or JudgeErrors, recorded per call."""

    def __init__(self, *script, gate="plan"):
        self.script = list(script)
        self.bodies = []
        self.gate = gate

    def __call__(self, body, api_key, timeout):
        self.bodies.append(json.loads(json.dumps(body)))
        step = self.script.pop(0) if self.script else self.default()
        if isinstance(step, Exception):
            raise step
        return step

    def default(self):
        return {"model": "jev-1.13.0", "answers": dev_answers() if self.gate == "dev" else plan_answers()}


def http(status, text="", retry_after=None):
    return jj.http_error(status, text, retry_after)


class Judged:
    """Run `judge` against a stubbed transport with the opt-in satisfied; exposes the run and call log."""

    def __init__(self, d, transport, anchors=None, extra_args=(), item_text="Sprint 2 copies the legacy runtime crates.", env_values=None, gate="plan"):
        self.anchors_path = write(d, "anchors.json", record(*(anchors or [anchor()])))
        self.item_path = write(d, "item.txt", item_text)
        self.sleeps = []
        argv = ["judge", "--anchors", self.anchors_path, "--gate", gate, "--all", "--item-id", "bd-42",
                "--item-file", self.item_path, "--run-dir", str(Path(d) / "runs"), *extra_args]
        with env(**(LIVE_ENV if env_values is None else env_values)), mock.patch.object(jj, "call_jev", transport), \
                mock.patch.object(jj, "_sleep", self.sleeps.append):
            self.code, self.out = run_cli(argv)
        self.run = self.out.get("data") if isinstance(self.out, dict) else None
        self.result = self.run["results"][0] if self.run and self.run.get("results") else None


# --------------------------------------------------------------------------- #
class AddTests(unittest.TestCase):
    def test_next_anc_id_allocation(self):
        self.assertEqual(jj.next_anc_id(record()), "ANC-001")
        rec = record(anchor("ANC-003"), anchor("ANC-010"), anchor("REQ-014", source_type="REQ", source_ref="REQ-014"), {"id": 7})
        self.assertEqual(jj.next_anc_id(rec), "ANC-011")
        self.assertEqual(jj.next_anc_id(record(anchor("ANC-1000"))), "ANC-1001")

    def test_add_creates_file_and_allocates_ids(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", "  The runtime crates must add no new dependency.  ", "--quote", " no new dependency for this ", "--status", "locked"])
            self.assertEqual(code, 0, out)
            self.assertEqual(out["data"]["anchor"]["id"], "ANC-001")
            self.assertEqual(out["data"]["anchor"]["statement"], "The runtime crates must add no new dependency.")
            self.assertEqual(out["data"]["anchor"]["quote"], "no new dependency for this")
            self.assertEqual(out["data"]["anchor"]["stated_at"], _dt.date.today().isoformat())
            self.assertEqual((out["data"]["lint"], out["data"]["similar"]), ({"warnings": []}, []))
            code, out = run_cli(["add", "--anchors", path, "--statement", "The CLI layer only parses arguments and prints JSON.", "--quote", "keep the CLI thin"])
            self.assertEqual(out["data"]["anchor"]["id"], "ANC-002")
            self.assertFalse(Path(path + ".tmp").exists())
            self.assertEqual(len(json.loads(Path(path).read_text())["anchors"]), 2)

    def test_register_anchors_keep_their_id_and_refuse_duplicates(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", "x", "--source-type", "REQ"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            code, out = run_cli(["add", "--anchors", path, "--statement", "x", "--source-type", "REQ", "--source-ref", "REQ-014"])
            self.assertEqual(out["data"]["anchor"]["id"], "REQ-014")
            code, out = run_cli(["add", "--anchors", path, "--statement", "x", "--source-type", "REQ", "--source-ref", "REQ-014"])
            self.assertEqual((code, out["error"]["code"]), (2, "ANCHOR.DUPLICATE"))

    def test_add_refuses_empty_statement_and_retired_status(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", "   "])
            self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")
            code, out = run_cli(["add", "--anchors", path, "--statement", "x", "--status", "retired"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))


class QuoteTests(unittest.TestCase):
    """The statement is authored and testable; the quote is the user's verbatim words."""

    def test_anc_requires_a_quote(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            self.assertIn("--quote is required", out["error"]["message"])
            self.assertFalse(Path(path).exists())
            code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT, "--quote", "   "])
            self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")
            code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT, "--quote", QUOTE])
            self.assertEqual((code, out["data"]["anchor"]["quote"]), (0, QUOTE))

    def test_register_anchors_default_the_quote_to_the_statement(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", REQ_STATEMENT, "--source-type", "REQ", "--source-ref", "REQ-014"])
            self.assertEqual((code, out["data"]["anchor"]["quote"], out["data"]["lint"]), (0, REQ_STATEMENT, {"warnings": []}))
            code, out = run_cli(["add", "--anchors", path, "--statement", "The CLI layer only parses arguments and prints JSON.", "--quote", "The CLI stays thin",
                                 "--source-type", "ADR", "--source-ref", "ADR-0007"])
            self.assertEqual(out["data"]["anchor"]["quote"], "The CLI stays thin")
            self.assertEqual(jj.validate_record(json.loads(Path(path).read_text())), [])

    def test_statement_equal_to_quote_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            for same in (QUOTE, QUOTE.upper(), "  " + QUOTE + ".  ", QUOTE.replace(" ", "   ") + "!"):
                code, out = run_cli(["add", "--anchors", path, "--statement", same, "--quote", QUOTE])
                self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"), same)
                self.assertEqual(out["error"]["message"], "statement is the verbatim quote; write a testable anchor (see references/anchor-capture.md)")
            self.assertFalse(Path(path).exists())
            # the same words are fine for register anchors: the quote is the register text
            code, out = run_cli(["add", "--anchors", path, "--statement", REQ_STATEMENT, "--quote", REQ_STATEMENT, "--source-type", "REQ", "--source-ref", "REQ-014"])
            self.assertEqual(code, 0, out)

    def test_validate_reports_missing_quote_and_verbatim_statement(self):
        old = anchor()
        del old["quote"]
        verbatim = anchor("ANC-002", statement=QUOTE.capitalize() + ".")
        reg = anchor("REQ-014", source_type="REQ", source_ref="REQ-014", statement=REQ_STATEMENT)
        del reg["quote"]
        problems = jj.validate_record(record(old, verbatim, reg, anchor("ANC-003", quote=7)))
        self.assertIn("ANC-001: quote missing; record the user's verbatim words", problems)
        self.assertIn("ANC-001: statement is the verbatim quote; write a testable anchor (see references/anchor-capture.md)", problems)
        self.assertIn("ANC-002: statement is the verbatim quote; write a testable anchor (see references/anchor-capture.md)", problems)
        self.assertIn("ANC-003: quote must be a string", problems)
        self.assertEqual([p for p in problems if p.startswith("REQ-014")], [])  # register text needs no separate quote
        self.assertEqual(jj.validate_record(record(anchor(), reg)), [])

    def test_records_without_quote_are_read_as_quote_equals_statement(self):
        old = anchor(statement=QUOTE)
        del old["quote"]
        self.assertEqual(jj.anchor_quote(old), QUOTE)
        self.assertEqual(jj.anchor_quote({"statement": "s", "quote": "  "}), "s")
        self.assertEqual(jj.anchor_quote(anchor()), QUOTE)
        self.assertEqual(jj.render_md(record(old)).count(f"> {QUOTE}"), 1)
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(old))
            code, out = run_cli(["lint", "--anchors", path])
            self.assertEqual((code, out["data"]["anchors"][0]["warnings"][0]), (1, "statement is the verbatim quote"))
            code, out = run_cli(["validate", "--anchors", path])
            self.assertEqual((code, out["error"]["code"]), (1, "VALIDATION.ANCHORS"))
            # an old run file: the report falls back to the statement as the quote
            run = make_run(d, "old-run.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "not_addressed", "REQ-014": "contradicted"}, with_quote=False)
            code, md = run_cli(["report", "--run", run, "--format", "md"])
            self.assertIn(f'Anchor: {STATEMENT} You said (planning session, Oct 9): "{STATEMENT}" This bead', md)
            self.assertIn(f"Anchor: {REQ_STATEMENT} This bead proposes", md)
            self.assertNotIn("says:", md)

    def test_run_results_carry_the_quote(self):
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(), anchors=[anchor(), anchor("REQ-014", source_type="REQ", source_ref="REQ-014", statement=REQ_STATEMENT)])
            by_id = {r["anchor_id"]: r for r in j.run["results"]}
            self.assertEqual((by_id["ANC-001"]["statement"], by_id["ANC-001"]["quote"]), (STATEMENT, QUOTE))
            self.assertEqual(by_id["REQ-014"]["quote"], REQ_STATEMENT)
            # Jev still sees only the statement and scope
            self.assertEqual(set(j.run["results"][0].keys()) >= {"statement", "quote"}, True)
            fixture = write(d, "answers.json", {"*": plan_answers()})
            anchors_path = write(d, "a.json", record(anchor()))
            item = write(d, "item.txt", "Sprint 2 copies the legacy crates.")
            with env():
                code, out = run_cli(["judge", "--anchors", anchors_path, "--gate", "plan", "--all", "--item-id", "bd-1", "--item-file", item, "--dry-run",
                                     "--out", str(Path(d) / "dry.json")])
            self.assertEqual(out["data"]["requests"][0]["state"]["anchor"], {"id": "ANC-001", "statement": STATEMENT, "scope": "the runtime crates"})
            with env():
                code, out = run_cli(["check-fixtures", "--anchors", write(d, "f.json", record(anchor(fixtures={"pass": ["copies the legacy repo"], "fail": []}))),
                                     "--gate", "plan", "--all", "--answers-file", fixture])
            self.assertEqual(out["data"]["checks"][0]["verdict"], "satisfied")

    def test_shipped_example_record_is_valid_lint_clean_and_renders(self):
        example = json.loads((ASSETS / "examples" / "anchors.example.json").read_text())
        self.assertEqual(jj.validate_record(example), [])
        by_id = {a["id"]: a for a in example["anchors"]}
        self.assertEqual(by_id["ANC-012"]["quote"], "copy the libraries from the legacy repo as the starting point and remove the legacy-specific code")
        self.assertTrue(by_id["ANC-012"]["statement"].startswith("The runtime crates are created by copying the legacy repo's runtime crates"))
        self.assertEqual(by_id["REQ-014"]["quote"], by_id["REQ-014"]["statement"])
        for a in example["anchors"]:
            self.assertEqual(jj.lint_statement(a["statement"], a["quote"], a["source_type"]), [], a["id"])
        md = jj.render_md(example)
        self.assertIn(f"Anchor: {by_id['ANC-012']['statement']}\n\n> {by_id['ANC-012']['quote']}", md)


class LintTests(unittest.TestCase):
    def lint(self, statement, quote=QUOTE, source_type="ANC"):
        return jj.lint_statement(statement, quote, source_type)

    def test_clean_statements_have_no_warnings(self):
        for s in (STATEMENT, REQ_STATEMENT, "The CLI layer only parses arguments and prints JSON.",
                  "crates/runtime is copied from the legacy repo/crates/runtime.", "The sprint never adds a dependency on tokio.",
                  "The parser does not call the network.", "Sprint 2 requires the legacy crates as its starting point."):
            self.assertEqual(self.lint(s), [], s)

    def test_verbatim_quote_rule_is_anc_only(self):
        self.assertEqual(self.lint(QUOTE)[0], "statement is the verbatim quote")
        self.assertEqual(self.lint("  " + QUOTE.upper() + "?!")[0], "statement is the verbatim quote")
        self.assertNotIn("statement is the verbatim quote", self.lint(REQ_STATEMENT, REQ_STATEMENT, "REQ"))
        self.assertNotIn("statement is the verbatim quote", self.lint(REQ_STATEMENT, REQ_STATEMENT, "ADR"))
        self.assertNotIn("statement is the verbatim quote", self.lint(STATEMENT, ""))

    def test_too_long(self):
        long = "The runtime crate is " + "x" * 230 + "."
        self.assertIn("too long; split into one constraint per anchor", self.lint(long))
        self.assertNotIn("too long; split into one constraint per anchor", self.lint("The runtime crate is " + "x" * 200 + "."))

    def test_bundled_constraints(self):
        self.assertIn("may bundle two constraints", self.lint("The crates must be copied and must never be rewritten."))
        self.assertIn("may bundle two constraints", self.lint("The CLI only prints JSON and always exits 0."))
        self.assertIn("may bundle two constraints", self.lint("The CLI must not fork; it shall exec."))
        self.assertNotIn("may bundle two constraints", self.lint("The CLI must not fork."))
        self.assertNotIn("may bundle two constraints", self.lint("The mustard module is copied."))  # word boundaries

    def test_hedges(self):
        for s in ("The crates should be copied.", "The crates could be copied.", "The crates are copied, maybe.", "The crates are probably copied.",
                  "The build must try to use the cache.", "The build is cached as needed.", "The CLI is thin if possible.", "Tests are run where appropriate.",
                  "The crate is copied from the legacy repo etc.", "The CLI must ideally print JSON."):
            self.assertIn("hedged wording; state the requirement", self.lint(s), s)
        self.assertNotIn("hedged wording; state the requirement", self.lint("The shoulder module is copied."))

    def test_speech(self):
        for s in ("We must copy the crates.", "You must copy the crates.", "I never rewrite crates.", "My crates are copied.", "Our CLI only prints JSON.",
                  "The crates are copied for me.", "The crates are given to us.", "Your crates are copied."):
            self.assertIn("written as speech, not as a requirement", self.lint(s), s)
        self.assertNotIn("written as speech, not as a requirement", self.lint("The usb module is copied; the yourt crate is not."))  # boundaries
        self.assertNotIn("written as speech, not as a requirement", self.lint("The weather service is never polled."))

    def test_requirement_verb(self):
        self.assertIn("not phrased as a testable requirement", self.lint("Copy the crates from the legacy repo."))
        self.assertIn("not phrased as a testable requirement", self.lint("A thin CLI with JSON output."))
        for s in ("The crates must be copied.", "Crates are copied.", "The CLI is thin.", "The CLI never forks.", "The CLI always exits 0.",
                  "The CLI only prints JSON.", "The CLI shall print JSON.", "The build requires cargo.", "The build uses cargo.", "The CLI does not fork."):
            self.assertNotIn("not phrased as a testable requirement", self.lint(s), s)

    def test_vague_terms(self):
        for s in ("The CLI must have appropriate output.", "The CLI must use proper JSON.", "Errors are properly reported.", "The code is clean.",
                  "The tests are good.", "The output is nice.", "Latency is reasonable.", "The parser is robust.", "The design is simple."):
            self.assertIn("vague term; name the observable outcome", self.lint(s), s)
        self.assertNotIn("vague term; name the observable outcome", self.lint("The cleanup module is deleted; the goodwill crate is kept."))

    def test_rules_fire_once_each_in_fixed_order(self):
        warnings = self.lint("We should probably keep things clean and proper, etc. " + "x" * 240)
        self.assertEqual(warnings, ["too long; split into one constraint per anchor", "hedged wording; state the requirement",
                                    "written as speech, not as a requirement", "not phrased as a testable requirement", "vague term; name the observable outcome"])

    def test_lint_subcommand_exit_codes_and_selection(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(), anchor("ANC-002", statement="We should keep it clean."),
                                                    anchor("ANC-003", status="retired", statement="Copy stuff."),
                                                    anchor("REQ-014", source_type="REQ", source_ref="REQ-014", statement=REQ_STATEMENT)))
            code, out = run_cli(["lint", "--anchors", path, "--all"])
            self.assertEqual(code, 1)
            self.assertEqual([r["id"] for r in out["data"]["anchors"]], ["ANC-001", "ANC-002", "REQ-014"])  # retired skipped
            self.assertEqual(out["data"]["anchors"][0]["warnings"], [])
            self.assertEqual(out["data"]["anchors"][1]["warnings"], ["hedged wording; state the requirement", "written as speech, not as a requirement",
                                                                      "not phrased as a testable requirement", "vague term; name the observable outcome"])
            self.assertEqual(out["data"]["warning_count"], 4)
            code, out = run_cli(["lint", "--anchors", path])  # neither flag: same as --all
            self.assertEqual((code, out["data"]["warning_count"]), (1, 4))
            code, out = run_cli(["lint", "--anchors", path, "--id", "ANC-001"])
            self.assertEqual((code, out["data"]), (0, {"anchors": [{"id": "ANC-001", "warnings": []}], "warning_count": 0, "path": path}))
            code, out = run_cli(["lint", "--anchors", path, "--id", "ANC-003"])  # --id reaches retired anchors
            self.assertEqual((code, out["data"]["warning_count"]), (1, 1))
            code, out = run_cli(["lint", "--anchors", path, "--id", "ANC-001", "--all"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            code, out = run_cli(["lint", "--anchors", path, "--id", "ANC-999"])
            self.assertEqual((code, out["error"]["code"]), (2, "ANCHOR.UNKNOWN"))

    def test_add_reports_warnings_and_strict_refuses(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", "We should keep the CLI thin.", "--quote", "keep the CLI thin", "--strict"])
            self.assertEqual((code, out["error"]["code"], out["error"]["recoverable"]), (2, "ANCHOR.LINT", True))
            self.assertIn("hedged wording", out["error"]["message"])
            self.assertFalse(Path(path).exists())
            code, out = run_cli(["add", "--anchors", path, "--statement", "We should keep the CLI thin.", "--quote", "keep the CLI thin"])
            self.assertEqual(code, 0)
            self.assertEqual(out["data"]["lint"]["warnings"], ["hedged wording; state the requirement", "written as speech, not as a requirement",
                                                               "not phrased as a testable requirement"])


class DedupeTests(unittest.TestCase):
    A = "The runtime crates are copied from the legacy repo's runtime crates with the legacy-specific modules deleted."

    def test_tokens_and_jaccard(self):
        self.assertEqual(jj.tokens("The CLI's --json output is stable, i.e. versioned."), {"cli", "json", "output", "stable", "versioned"})
        self.assertEqual(jj.jaccard(set(), set()), 0.0)
        self.assertEqual(jj.jaccard({"a", "b"}, {"b", "c"}), 1 / 3)
        score, fa, fb = jj.best_similarity(STATEMENT, QUOTE, self.A, "copy the libraries from the legacy repo and remove the legacy-specific code")
        self.assertEqual((fa, fb), ("quote", "quote"))
        self.assertGreater(score, 0.7)
        self.assertEqual(jj.best_similarity("", "", "", "")[0], 0.0)

    def test_deterministic_pairs_and_exit_code(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(
                anchor(),
                anchor("ANC-002", statement=self.A, quote="copy the libraries from the legacy repo as a starting point and remove the legacy-specific code"),
                anchor("ANC-003", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin"),
                anchor("ANC-004", status="retired", statement=STATEMENT),
                anchor("REQ-014", source_type="REQ", source_ref="REQ-014", statement=REQ_STATEMENT)))
            code, out = run_cli(["dedupe", "--anchors", path])
            self.assertEqual(code, 1, out)
            data = out["data"]
            self.assertEqual((data["threshold"], data["jev"], data["anchors"]), (0.5, False, 4))
            self.assertEqual([(p["a"], p["b"]) for p in data["pairs"]], [("ANC-001", "ANC-002")])
            pair = data["pairs"][0]
            self.assertEqual((pair["field_a"], pair["field_b"]), ("quote", "quote"))
            self.assertEqual(set(pair), {"a", "b", "jaccard", "field_a", "field_b"})
            self.assertEqual(data["likely_duplicates"], data["pairs"])  # jaccard >= 0.8 without --jev
            self.assertEqual(pair["jaccard"], 1.0)  # same tokens after stopwords ("the" vs "a")
            distinct = write(d, "distinct.json", record(anchor(), anchor("ANC-003", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin")))
            code, out = run_cli(["dedupe", "--anchors", distinct])
            self.assertEqual((code, out["data"]["pairs"], out["data"]["likely_duplicates"]), (0, [], []))
            code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "0.2"])
            self.assertEqual(code, 1)
            self.assertGreaterEqual(len(out["data"]["pairs"]), 1)
            self.assertEqual(len(out["data"]["likely_duplicates"]), 1)
            self.assertEqual(out["data"]["pairs"][0]["jaccard"], max(p["jaccard"] for p in out["data"]["pairs"]))  # best first
            code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "1.5"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))

    def test_jev_pass_with_answers_file_dry_run_and_opt_in(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(
                anchor(),
                anchor("ANC-002", statement=self.A, quote="copy the libraries from the legacy repo as a starting point and remove the legacy-specific code"),
                anchor("ANC-003", statement="The runtime crates are copied from the legacy repo.", quote="copy the runtime crates from the legacy repo")))
            yes = {"same_constraint": answer("yes", 0.9)}
            no = {"same_constraint": answer("no", 0.9)}
            weak_yes = {"same_constraint": answer("yes", 0.55)}
            fixture = write(d, "answers.json", {"ANC-001|ANC-002": yes, "ANC-001|ANC-003": no, "*": weak_yes})
            with env():
                code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "0.25", "--answers-file", fixture])
            self.assertEqual(code, 1, out)
            data = out["data"]
            self.assertTrue(data["jev"] and data["fixture"])
            by_pair = {(p["a"], p["b"]): p for p in data["pairs"]}
            self.assertEqual(by_pair[("ANC-001", "ANC-002")]["jev"], {"choice": "yes", "confidence": 0.9, "attempts": 1})
            self.assertEqual(by_pair[("ANC-001", "ANC-003")]["jev"]["choice"], "no")
            self.assertEqual(by_pair[("ANC-002", "ANC-003")]["jev"]["confidence"], 0.55)  # '*' fallback, below the confidence threshold
            self.assertEqual([(p["a"], p["b"]) for p in data["likely_duplicates"]], [("ANC-001", "ANC-002")])
            # a pair the fixture lacks is an error on that pair, not a crash
            partial = write(d, "partial.json", {"ANC-001|ANC-002": no})
            with env():
                code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "0.25", "--answers-file", partial])
            self.assertEqual(code, 0)
            errors = [p for p in out["data"]["pairs"] if p["jev"].get("error")]
            self.assertEqual({e["jev"]["error"]["code"] for e in errors}, {"FIXTURE.MISSING"})
            # dry run: requests are shown, nothing is called, the deterministic rule decides
            with env():
                code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "0.25", "--jev", "--dry-run"])
            self.assertEqual(code, 1)
            req = out["data"]["requests"][0]
            self.assertEqual(set(req), {"model", "questions", "state"})
            self.assertEqual(set(req["state"]), {"a", "b"})
            self.assertEqual(set(req["state"]["a"]), {"id", "statement", "quote"})
            self.assertEqual(set(req["questions"]), {"same_constraint"})
            self.assertEqual(out["data"]["totals"]["requests"], len(out["data"]["pairs"]))
            self.assertEqual([(p["a"], p["b"]) for p in out["data"]["likely_duplicates"]], [("ANC-001", "ANC-002")])
            self.assertTrue(all(p["jev"] is None for p in out["data"]["pairs"]))
            self.assertEqual(sorted(Path(d).iterdir()), sorted(Path(d) / n for n in ("anchors.json", "answers.json", "partial.json")))  # no run files
            # live: key and opt-in are enforced before anything is sent
            with env():
                code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "0.25", "--jev"])
            self.assertEqual((code, out["error"]["code"]), (2, "AUTH.CREDENTIALS"))
            with env(TYPESAFE_API_KEY="sk-test"):
                code, out = run_cli(["dedupe", "--anchors", path, "--threshold", "0.25", "--jev"])
            self.assertEqual((code, out["error"]["code"]), (2, "POLICY.REMOTE_JUDGE_DISABLED"))
            self.assertIn("statements and quotes", out["error"]["message"])
            distinct = write(d, "distinct.json", record(anchor(), anchor("ANC-003", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin")))
            with env():
                code, out = run_cli(["dedupe", "--anchors", distinct, "--jev"])  # no pairs: nothing to send, no key needed
            self.assertEqual((code, out["data"]["pairs"], out["data"]["jev"]), (0, [], True))

    def test_jev_live_uses_the_transport_policy(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(), anchor("ANC-002", statement=self.A, quote="copy the libraries from the legacy repo and remove the legacy-specific code")))
            t = Transport(http(429), {"model": "jev-1.13.0", "answers": {"same_constraint": answer("yes", 0.8)}})
            sleeps = []
            with env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t), mock.patch.object(jj, "_sleep", sleeps.append):
                code, out = run_cli(["dedupe", "--anchors", path, "--jev"])
            self.assertEqual(code, 1)
            self.assertEqual(out["data"]["pairs"][0]["jev"], {"choice": "yes", "confidence": 0.8, "attempts": 2})
            self.assertEqual((len(t.bodies), sleeps), (2, [1.0]))
            self.assertEqual(t.bodies[0]["state"]["a"]["id"], "ANC-001")
            t = Transport(http(401))
            with env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t):
                code, out = run_cli(["dedupe", "--anchors", path, "--jev"])
            self.assertEqual((code, out["error"]["code"]), (2, "AUTH.CREDENTIALS"))
            t = Transport(http(503), http(503), http(503))
            with env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t), mock.patch.object(jj, "_sleep", lambda s: None):
                code, out = run_cli(["dedupe", "--anchors", path, "--jev"])
            self.assertEqual(code, 0)
            self.assertEqual((out["data"]["pairs"][0]["jev"]["error"]["code"], out["data"]["pairs"][0]["jev"]["attempts"]), ("JEV.HTTP", 3))

    def test_add_refuses_duplicate_candidates_unless_allowed(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(), anchor("ANC-002", status="retired", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin")))
            # similar quote to the locked anchor
            code, out = run_cli(["add", "--anchors", path, "--statement", self.A, "--quote", "copy the libraries from the legacy repo and remove the legacy-specific code"])
            self.assertEqual((code, out["error"]["code"], out["error"]["recoverable"]), (2, "ANCHOR.DUPLICATE_CANDIDATE", True))
            self.assertIn("ANC-001", out["error"]["message"])
            self.assertIn("ANC-001", out["error"]["suggested_action"])
            self.assertIn("--allow-similar", out["error"]["suggested_action"])
            self.assertEqual(len(json.loads(Path(path).read_text())["anchors"]), 2)
            # similar statement to the locked anchor's statement, with an unrelated quote
            code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT.replace("deleting", "removing"), "--quote", "take the legacy repo's code and strip it"])
            self.assertEqual(out["error"]["code"], "ANCHOR.DUPLICATE_CANDIDATE")
            # a retired anchor never blocks
            code, out = run_cli(["add", "--anchors", path, "--statement", "The CLI layer only parses arguments and prints JSON.", "--quote", "keep the CLI thin"])
            self.assertEqual((code, out["data"]["anchor"]["id"]), (0, "ANC-003"))
            self.assertEqual(out["data"]["similar"], [])
            # --allow-similar records it and reports the candidates
            code, out = run_cli(["add", "--anchors", path, "--statement", self.A, "--quote", "copy the libraries from the legacy repo and remove the legacy-specific code", "--allow-similar"])
            self.assertEqual((code, out["data"]["anchor"]["id"]), (0, "ANC-004"))
            self.assertEqual(out["data"]["similar"][0]["id"], "ANC-001")
            self.assertGreaterEqual(out["data"]["similar"][0]["jaccard"], 0.6)
            self.assertEqual(set(out["data"]["similar"][0]), {"id", "jaccard", "field"})
            self.assertEqual(out["data"]["similar"][0]["field"], "quote/quote")


def rubric_answers(**overrides):
    answers = {q: answer("yes", 0.9) for q in jj.RUBRIC_QUESTIONS}
    for q, (choice, p) in overrides.items():
        answers[q] = answer(choice, p)
    return answers


class RubricTests(unittest.TestCase):
    def test_positive_form_is_the_sixth_question(self):
        self.assertEqual(jj.RUBRIC_QUESTIONS, ("single_constraint", "observable", "requirement_form", "faithful", "scoped", "positive_form"))
        q = jj.load_simple_question_set("testability", jj.RUBRIC_QUESTIONS)["questions"]["positive_form"]
        self.assertIn("unless", q["instructions"])
        self.assertIn("conditional", q["criteria"]["no"])
        with tempfile.TemporaryDirectory() as d:
            fixture = write(d, "answers.json", {"candidate": rubric_answers(positive_form=("no", 0.9))})
            with env():
                code, out = run_cli(["rubric", "--statement", "The build must not add a dependency unless the lead approved it.", "--quote", "no new deps unless I say so",
                                     "--answers-file", fixture])
            self.assertEqual((code, out["data"]["anchors"][0]["verdict"], out["data"]["anchors"][0]["failing"]), (1, "needs_work", ["positive_form"]))
            five = {q: answer("yes", 0.9) for q in jj.RUBRIC_QUESTIONS[:5]}  # an old five-question fixture no longer validates
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE, "--answers-file", write(d, "five.json", {"*": five})])
            self.assertEqual((out["data"]["anchors"][0]["verdict"], out["data"]["anchors"][0]["error"]["code"]), ("error", "JEV.RESPONSE_INVALID"))

    def test_shipped_question_sets_load_and_fixtures_validate(self):
        for name, required in (("dedupe", (jj.DEDUPE_QUESTION,)), ("testability", jj.RUBRIC_QUESTIONS)):
            qs = jj.load_simple_question_set(name, required)
            self.assertEqual(qs["gate"], name)
            self.assertEqual(set(qs["questions"]), set(required))
            for q in qs["questions"].values():
                self.assertEqual((q["type"], set(q["criteria"])), ("choice", set(OPTIONS)))
                self.assertTrue(q["instructions"].strip())
        jj.validate_response({"answers": rubric_answers()}, jj.load_simple_question_set("testability", jj.RUBRIC_QUESTIONS)["questions"])
        with mock.patch.object(jj, "QUESTIONS_DIR", Path("/nonexistent")):
            with self.assertRaises(jj.JudgeError) as cm:
                jj.load_simple_question_set("dedupe", (jj.DEDUPE_QUESTION,))
            self.assertEqual(cm.exception.code, "INPUT.MISSING")

    def test_record_form_with_answers_file(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(), anchor("ANC-002", statement="We should keep it clean.", quote="keep it clean"),
                                                    anchor("ANC-003", status="retired"), anchor("ANC-004", statement=QUOTE.capitalize())))
            fixture = write(d, "answers.json", {"ANC-002": rubric_answers(observable=("no", 0.9), requirement_form=("yes", 0.5)), "*": rubric_answers()})
            with env():
                code, out = run_cli(["rubric", "--anchors", path, "--all", "--answers-file", fixture])
            self.assertEqual(code, 1, out)
            data = out["data"]
            self.assertEqual((data["threshold"], data["dry_run"], data["fixture"], data["path"], data["pass"]), (0.6, False, True, path, False))
            by_id = {e["id"]: e for e in data["anchors"]}
            self.assertEqual(list(by_id), ["ANC-001", "ANC-002", "ANC-004"])
            self.assertEqual((by_id["ANC-001"]["verdict"], by_id["ANC-001"]["failing"], by_id["ANC-001"]["statement"], by_id["ANC-001"]["lint"]),
                             ("testable", [], STATEMENT, {"warnings": [], "errors": []}))
            self.assertEqual(set(by_id["ANC-001"]["answers"]), set(jj.RUBRIC_QUESTIONS))
            self.assertEqual((by_id["ANC-002"]["verdict"], by_id["ANC-002"]["failing"]), ("needs_work", ["observable", "requirement_form"]))
            self.assertIn("hedged wording; state the requirement", by_id["ANC-002"]["lint"]["warnings"])  # warnings ride along without failing
            # the verbatim rule is a lint error: it fails the rubric even when Jev says yes to everything
            self.assertEqual((by_id["ANC-004"]["verdict"], by_id["ANC-004"]["failing"]), ("needs_work", ["statement is the verbatim quote"]))
            self.assertEqual(data["needs_work"], ["ANC-002", "ANC-004"])
            with env():
                code, out = run_cli(["rubric", "--anchors", path, "--id", "ANC-001", "--answers-file", fixture])
            self.assertEqual((code, out["data"]["pass"], [e["id"] for e in out["data"]["anchors"]]), (0, True, ["ANC-001"]))
            with env():
                code, out = run_cli(["rubric", "--anchors", path, "--id", "ANC-001", "--answers-file", fixture, "--threshold", "0.95"])
            self.assertEqual((code, out["data"]["anchors"][0]["verdict"]), (1, "needs_work"))
            self.assertEqual(out["data"]["anchors"][0]["failing"], list(jj.RUBRIC_QUESTIONS))
            missing = write(d, "missing.json", {"ANC-002": rubric_answers()})
            with env():
                code, out = run_cli(["rubric", "--anchors", path, "--id", "ANC-001", "--answers-file", missing])
            self.assertEqual((code, out["data"]["anchors"][0]["verdict"], out["data"]["anchors"][0]["error"]["code"]), (1, "error", "FIXTURE.MISSING"))
            self.assertEqual(out["data"]["anchors"][0]["failing"], ["error: FIXTURE.MISSING"])
            self.assertEqual(sorted(p.name for p in Path(d).iterdir()), ["anchors.json", "answers.json", "missing.json"])  # no run files

    def test_candidate_form_dry_run_and_opt_in(self):
        with tempfile.TemporaryDirectory() as d:
            fixture = write(d, "answers.json", {"candidate": rubric_answers(faithful=("no", 0.8))})
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE, "--scope", "runtime crates", "--answers-file", fixture])
            self.assertEqual(code, 1, out)
            entry = out["data"]["anchors"][0]
            self.assertEqual((entry["id"], entry["verdict"], entry["failing"], entry["scope"], out["data"]["path"]), ("candidate", "needs_work", ["faithful"], "runtime crates", None))
            star = write(d, "star.json", {"*": rubric_answers()})
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE, "--answers-file", star])
            self.assertEqual((code, out["data"]["pass"]), (0, True))
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE, "--dry-run"])
            self.assertEqual(code, 0)
            self.assertEqual((out["data"]["dry_run"], out["data"]["pass"], out["data"]["anchors"][0]["verdict"]), (True, False, None))
            req = out["data"]["requests"][0]
            self.assertEqual(req["state"], {"anchor": {"statement": STATEMENT, "quote": QUOTE, "scope": ""}})
            self.assertEqual(set(req["questions"]), set(jj.RUBRIC_QUESTIONS))
            self.assertEqual(out["data"]["totals"]["requests"], 1)
            # input checks
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE, "--anchors", "x.json"])
            self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")
            with env():
                code, out = run_cli(["rubric", "--dry-run"])
            self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")
            with env():
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE])
            self.assertEqual((code, out["error"]["code"]), (2, "AUTH.CREDENTIALS"))
            with env(TYPESAFE_API_KEY="sk-test"):
                code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE])
            self.assertEqual((code, out["error"]["code"]), (2, "POLICY.REMOTE_JUDGE_DISABLED"))

    def test_live_candidate_uses_the_transport(self):
        t = Transport({"model": "jev-1.13.0", "answers": rubric_answers(scoped=("unclear", 0.7))})
        with env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t):
            code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE, "--scope", "runtime crates"])
        self.assertEqual(code, 1)
        self.assertEqual((out["data"]["anchors"][0]["verdict"], out["data"]["anchors"][0]["failing"], out["data"]["anchors"][0]["attempts"]), ("needs_work", ["scoped"], 1))
        self.assertEqual(t.bodies[0]["state"]["anchor"]["scope"], "runtime crates")
        self.assertNotIn("item", t.bodies[0]["state"])
        # a state without an item cannot be shortened: over the cap it is refused before any call
        with mock.patch.object(jj, "DEFAULT_MAX_REQUEST_BYTES", 200), env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t):
            code, out = run_cli(["rubric", "--statement", STATEMENT, "--quote", QUOTE])
        self.assertEqual((code, out["data"]["anchors"][0]["verdict"], out["data"]["anchors"][0]["error"]["code"]), (1, "error", "JEV.CONTEXT_EXCEEDED"))
        self.assertEqual(len(t.bodies), 1)

    def test_add_rubric_gate(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            bad = write(d, "bad.json", {"candidate": rubric_answers(single_constraint=("no", 0.9), observable=("yes", 0.3))})
            good = write(d, "good.json", {"*": rubric_answers()})
            with env():
                code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT, "--quote", QUOTE, "--rubric", "--answers-file", bad])
            self.assertEqual((code, out["error"]["code"], out["error"]["recoverable"]), (2, "ANCHOR.NOT_TESTABLE", True))
            self.assertIn("single_constraint, observable", out["error"]["message"])
            self.assertIn("--allow-untestable", out["error"]["suggested_action"])
            self.assertFalse(Path(path).exists())
            with env():
                code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT, "--quote", QUOTE, "--rubric", "--answers-file", bad, "--allow-untestable"])
            self.assertEqual((code, out["data"]["anchor"]["id"], out["data"]["rubric"]["verdict"]), (0, "ANC-001", "needs_work"))
            self.assertEqual(out["data"]["rubric"]["failing"], ["single_constraint", "observable"])
            with env():
                code, out = run_cli(["add", "--anchors", path, "--statement", "The CLI layer only parses arguments and prints JSON.", "--quote", "keep the CLI thin",
                                     "--rubric", "--answers-file", good])
            self.assertEqual((code, out["data"]["anchor"]["id"], out["data"]["rubric"]["verdict"], out["data"]["rubric"]["id"]), (0, "ANC-002", "testable", "candidate"))
            # without --rubric nothing is asked and no key is needed; with it, the opt-in applies
            with env():
                code, out = run_cli(["add", "--anchors", path, "--statement", "The parser does not call the network.", "--quote", "no network in the parser"])
            self.assertEqual(code, 0)
            self.assertNotIn("rubric", out["data"])
            with env():
                code, out = run_cli(["add", "--anchors", path, "--statement", "The build uses cargo only.", "--quote", "cargo only", "--rubric"])
            self.assertEqual((code, out["error"]["code"]), (2, "AUTH.CREDENTIALS"))
            self.assertEqual(len(json.loads(Path(path).read_text())["anchors"]), 3)
            t = Transport({"model": "jev-1.13.0", "answers": rubric_answers()})
            with env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t):
                code, out = run_cli(["add", "--anchors", path, "--statement", "The build uses cargo only.", "--quote", "cargo only", "--rubric"])
            self.assertEqual((code, out["data"]["anchor"]["id"], out["data"]["rubric"]["verdict"]), (0, "ANC-004", "testable"))
            self.assertEqual(t.bodies[0]["state"]["anchor"]["quote"], "cargo only")


class KindTests(unittest.TestCase):
    """Anchor kind: directive (default), prohibition (compliance role ignored), structural (plan gate skips Jev)."""

    def test_add_update_validate_kind(self):
        with tempfile.TemporaryDirectory() as d:
            path = str(Path(d) / "anchors.json")
            code, out = run_cli(["add", "--anchors", path, "--statement", STATEMENT, "--quote", QUOTE])
            self.assertEqual((code, out["data"]["anchor"]["kind"]), (0, "directive"))
            code, out = run_cli(["add", "--anchors", path, "--statement", "The sprint never adds a dependency on tokio.", "--quote", "no tokio", "--kind", "prohibition"])
            self.assertEqual((code, out["data"]["anchor"]["kind"]), (0, "prohibition"))
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--kind", "structural"])
            self.assertEqual((code, out["data"]["changed"], out["data"]["version"]), (0, ["kind"], 2))
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--kind", "structural"])  # unchanged
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            rec = json.loads(Path(path).read_text())
            self.assertEqual([a["kind"] for a in rec["anchors"]], ["structural", "prohibition"])
            self.assertEqual(jj.validate_record(rec), [])
            rec["anchors"][0]["kind"] = "vibe"
            self.assertIn("ANC-001: kind must be one of ('directive', 'prohibition', 'structural')", jj.validate_record(rec))
            old = anchor()
            old.pop("kind", None)
            self.assertEqual((jj.anchor_kind(old), jj.validate_record(record(old))), ("directive", []))  # records without kind are directives
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                jj.main(["add", "--anchors", path, "--statement", "x", "--quote", "y", "--kind", "vibe"])

    def test_prohibition_gating_ignores_compliance(self):
        def pro(**kw):
            return jj.decide(plan_answers(**kw), PLAN_ROLES, 0.6, "prohibition")
        self.assertEqual(pro(rel="yes", comp="no", conf="no"), ("satisfied", 0.9))        # refrains: the directive answer would be not_addressed
        self.assertEqual(jj.decide(plan_answers(rel="yes", comp="no", conf="no"), PLAN_ROLES, 0.6), ("not_addressed", 0.9))
        self.assertEqual(pro(rel="no", comp="no", conf="no")[0], "not_applicable")
        self.assertEqual(pro(rel="yes", comp="no", conf="yes")[0], "contradicted")
        self.assertEqual(pro(rel="unclear", comp="no", conf="no")[0], "underspecified")
        self.assertEqual(pro(rel="yes", comp="no", conf="no", rel_p=0.5)[0], "underspecified")
        self.assertEqual(pro(rel="yes", comp="no", conf="no", conf_p=0.5)[0], "underspecified")
        self.assertEqual(pro(rel="yes", comp="no", conf="unclear")[0], "underspecified")
        self.assertEqual(pro(rel="yes", comp="unclear", conf="no", comp_p=0.1), ("satisfied", 0.9))  # compliance confidence never enters min_conf
        dev = jj.decide(dev_answers(comp="no", evid="no"), DEV_ROLES, 0.6, "prohibition")
        self.assertEqual(dev[0], "asserted_only")
        self.assertEqual(jj.decide(dev_answers(comp="no", evid="yes"), DEV_ROLES, 0.6, "prohibition")[0], "satisfied")
        self.assertEqual(jj.decide(plan_answers(), PLAN_ROLES, 0.6, "structural")[0], "satisfied")  # structural gates like a directive when judged

    def test_structural_skips_jev_at_the_plan_gate_only(self):
        with tempfile.TemporaryDirectory() as d:
            anchors = [anchor(), anchor("ANC-002", kind="structural", statement="Every sprint bead names one architectural layer (a)-(d).", quote="one layer per bead",
                                       deterministic_check="grep -c '^Layer:' on each bead")]
            t = Transport()
            j = Judged(d, t, anchors=anchors)
            self.assertEqual(j.code, 0)
            by_id = {r["anchor_id"]: r for r in j.run["results"]}
            self.assertEqual((by_id["ANC-002"]["verdict"], by_id["ANC-002"]["kind"], by_id["ANC-002"]["answers"], by_id["ANC-002"]["attempts"]),
                             ("deterministic_only", "structural", None, 0))
            self.assertEqual(by_id["ANC-002"]["explanation"], "Structural anchor: verified by its deterministic_check, not by Jev")
            self.assertEqual(by_id["ANC-001"]["kind"], "directive")
            self.assertEqual([b["state"]["anchor"]["id"] for b in t.bodies], ["ANC-001"])  # one request, not two
            self.assertEqual(j.run["summary"]["counts"], {"satisfied": 1, "deterministic_only": 1})
            self.assertTrue(j.run["summary"]["pass"])
            self.assertEqual([r["anchor_id"] for r in j.run["results"]], ["ANC-001", "ANC-002"])  # sorted after the passing verdicts
            code, md = run_cli(["report", "--run", j.run["path"], "--format", "md"])
            self.assertNotIn("ANC-002", md)  # non-failing: hidden without --all
            code, md = run_cli(["report", "--run", j.run["path"], "--format", "md", "--all"])
            self.assertIn("> **bd-42 · ANC-002 · deterministic only** Anchor: Every sprint bead names one architectural layer (a)-(d). You said", md)
            # dry run sends nothing for it either
            j = Judged(d, Transport(), anchors=anchors, extra_args=["--dry-run"])
            self.assertEqual((len(j.run["requests"]), {r["anchor_id"]: r["verdict"] for r in j.run["results"]}), (1, {"ANC-001": None, "ANC-002": "deterministic_only"}))
            # dev gate judges it normally
            t = Transport(gate="dev")
            j = Judged(d, t, anchors=anchors, gate="dev")
            self.assertEqual({r["anchor_id"]: r["verdict"] for r in j.run["results"]}, {"ANC-001": "satisfied", "ANC-002": "satisfied"})
            self.assertEqual(len(t.bodies), 2)
            # check-fixtures: a structural anchor's plan-gate fixtures are not checked, not mismatched
            fx = anchor("ANC-003", kind="structural", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin",
                        fixtures={"pass": ["The CLI parses and prints."], "fail": ["The CLI runs the linter itself."]})
            path = write(d, "fx.json", record(fx))
            answers = write(d, "answers.json", {"*": plan_answers()})
            with env():
                code, out = run_cli(["check-fixtures", "--anchors", path, "--gate", "plan", "--all", "--answers-file", answers])
            self.assertEqual((code, out["data"]["mismatches"], [c["verdict"] for c in out["data"]["checks"]], [c["expected_ok"] for c in out["data"]["checks"]]),
                             (0, [], ["deterministic_only", "deterministic_only"], [None, None]))
            self.assertEqual(jj.exit_code_for("judge", {"success": True, "data": {"dry_run": False, "summary": {"pass": True}}}), 0)
            self.assertEqual(jj.SEVERITY["deterministic_only"], jj.SEVERITY["not_applicable"] + 1)
            self.assertNotIn("deterministic_only", jj.FAILING)

    def test_prohibition_via_record_and_fixture(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(kind="prohibition", statement="The sprint never adds a dependency on tokio.", quote="no tokio")))
            item = write(d, "item.txt", "Sprint 2 copies the crates and adds no dependencies.")
            answers = write(d, "answers.json", {"*": plan_answers(comp="no", conf="no")})
            with env():
                code, out = run_cli(["judge", "--anchors", path, "--gate", "plan", "--all", "--item-id", "bd-1", "--item-file", item, "--answers-file", answers,
                                     "--run-dir", str(Path(d) / "runs")])
            self.assertEqual((code, out["data"]["results"][0]["verdict"], out["data"]["results"][0]["kind"]), (0, "satisfied", "prohibition"))
            with env():
                code, out = run_cli(["check-fixtures", "--anchors", write(d, "f.json", record(anchor(kind="prohibition", fixtures={"pass": ["adds nothing"], "fail": ["adds tokio"]}))),
                                     "--gate", "plan", "--all", "--answers-file", write(d, "a2.json", {"ANC-001:pass:1": plan_answers(comp="no", conf="no"), "ANC-001:fail:1": plan_answers(comp="no", conf="yes")})])
            self.assertEqual((code, [c["verdict"] for c in out["data"]["checks"]]), (0, ["satisfied", "contradicted"]))


class LeaningTests(unittest.TestCase):
    def test_leaning_is_the_verdict_by_choices_alone(self):
        self.assertEqual(jj.leaning_of(plan_answers(comp="no", p=0.3), PLAN_ROLES, "directive"), "not_addressed")
        self.assertEqual(jj.leaning_of(plan_answers(conf="yes", p=0.2), PLAN_ROLES, "directive"), "contradicted")
        self.assertEqual(jj.leaning_of(plan_answers(rel="no", p=0.1), PLAN_ROLES, "directive"), "not_applicable")
        self.assertEqual(jj.leaning_of(plan_answers(rel="unclear"), PLAN_ROLES, "directive"), "underspecified")
        self.assertEqual(jj.leaning_of(plan_answers(comp="no", p=0.3), PLAN_ROLES, "prohibition"), "satisfied")

    def test_underspecified_rows_show_the_lean(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(), anchor("ANC-002", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin")))
            item = write(d, "item.txt", "Sprint 2 copies the crates.")
            answers = write(d, "answers.json", {"ANC-001": plan_answers(comp="no", comp_p=0.45), "ANC-002": plan_answers()})
            with env():
                code, out = run_cli(["judge", "--anchors", path, "--gate", "plan", "--all", "--item-id", "bd-1", "--item-file", item, "--answers-file", answers,
                                     "--run-dir", str(Path(d) / "runs")])
            run = out["data"]
            by_id = {r["anchor_id"]: r for r in run["results"]}
            self.assertEqual((by_id["ANC-001"]["verdict"], by_id["ANC-001"]["leaning"]), ("underspecified", "not_addressed"))
            self.assertEqual((by_id["ANC-002"]["verdict"], by_id["ANC-002"]["leaning"]), ("satisfied", "satisfied"))
            self.assertEqual(run["summary"]["leaning_counts"], {"not_addressed": 1, "satisfied": 1})
            self.assertEqual(by_id["ANC-001"]["low_questions"],
                             [{"ask": "first", "role": "compliance", "question_id": "addressed", "choice": "no", "confidence": 0.45}])
            self.assertNotIn("low_questions", by_id["ANC-002"])
            code, md = run_cli(["report", "--run", run["path"], "--format", "md", "--all"])
            self.assertIn("the anchor or the bead may be underspecified. (leaning: not_addressed)", md)
            self.assertIn(">   Low: compliance `addressed` answered no at 0.45.", md)
            self.assertNotIn("(leaning: satisfied)", md)
            old = make_run(d, "old.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "underspecified"})  # pre-leaning run files
            code, md = run_cli(["report", "--run", old, "--format", "md"])
            self.assertNotIn("leaning", md)


class ConfirmTests(unittest.TestCase):
    """Near the threshold one answer is noise: the identical request is sent again and must agree."""

    def test_agreeing_rejudge_confirms(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.65)}, {"model": "jev", "answers": plan_answers(comp_p=0.72)})
            j = Judged(d, t)
        self.assertEqual((j.code, j.result["verdict"], j.result["attempts"], j.result["confirmed"], j.result["near_threshold"]), (0, "satisfied", 2, True, True))
        self.assertEqual(j.result["answers"]["addressed"]["confidence"], 0.65)  # the first answers are kept
        self.assertNotIn("answers_second", j.result)
        self.assertEqual(len(t.bodies), 2)
        self.assertEqual(t.bodies[0], t.bodies[1])  # identical request

    def test_disagreeing_rejudge_is_underspecified_and_unstable(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.62)}, {"model": "jev", "answers": plan_answers(comp="no", comp_p=0.7)})
            j = Judged(d, t)
            code, md = run_cli(["report", "--run", j.run["path"], "--format", "md"])
        self.assertEqual((j.code, j.result["verdict"], j.result["attempts"], j.result["unstable"]), (1, "underspecified", 2, True))
        self.assertEqual((j.result["verdict_first"], j.result["verdict_second"]), ("satisfied", "not_addressed"))
        self.assertEqual(j.result["answers"]["addressed"]["choice"], "yes")
        self.assertEqual(j.result["answers_second"]["addressed"]["choice"], "no")
        self.assertEqual(j.result["leaning"], "satisfied")
        self.assertEqual(j.run["summary"]["unstable"], ["ANC-001"])
        self.assertNotIn("confirmed", j.result)
        self.assertIn(">   Unstable: two identical requests gave satisfied then not_addressed.", md)
        self.assertIn("(leaning: satisfied)", md)

    def test_band_and_switches(self):
        with tempfile.TemporaryDirectory() as d:  # 0.71 is outside the 0.10 band: no re-judge
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.71)})
            j = Judged(d, t)
        self.assertEqual((j.result["attempts"], len(t.bodies)), (1, 1))
        self.assertNotIn("near_threshold", j.result)
        with tempfile.TemporaryDirectory() as d:  # exactly 0.10 away counts
            t = Transport({"model": "jev", "answers": plan_answers(rel_p=0.7)})
            j = Judged(d, t)
        self.assertEqual((j.result["attempts"], j.result["confirmed"]), (2, True))
        with tempfile.TemporaryDirectory() as d:  # --no-confirm
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.65)})
            j = Judged(d, t, extra_args=["--no-confirm"])
        self.assertEqual((j.result["attempts"], j.result["verdict"]), (1, "satisfied"))
        self.assertNotIn("near_threshold", j.result)
        with tempfile.TemporaryDirectory() as d:  # env off, flag on wins
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.65)})
            j = Judged(d, t, env_values=dict(LIVE_ENV, PLAN_ANCHOR_CONFIRM_NEAR="0"))
            self.assertEqual(j.result["attempts"], 1)
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.65)})
            j = Judged(d, t, env_values=dict(LIVE_ENV, PLAN_ANCHOR_CONFIRM_NEAR="0"), extra_args=["--confirm-near-threshold"])
            self.assertEqual(j.result["attempts"], 2)
        self.assertTrue(jj.confirm_enabled(jj.argparse.Namespace()))
        self.assertFalse(jj.confirm_enabled(jj.argparse.Namespace(no_confirm=True, confirm_near_threshold=True)))
        # a prohibition does not re-judge for a near-threshold compliance answer it ignores
        self.assertFalse(jj.near_threshold(plan_answers(comp_p=0.62), PLAN_ROLES, 0.6, "prohibition"))
        self.assertTrue(jj.near_threshold(plan_answers(comp_p=0.62), PLAN_ROLES, 0.6, "directive"))
        self.assertTrue(jj.near_threshold(dev_answers(evid_p=0.55), DEV_ROLES, 0.6, "prohibition"))

    def test_fixture_answers_are_never_rejudged_and_a_failed_rejudge_keeps_the_first(self):
        with tempfile.TemporaryDirectory() as d:
            fixture = write(d, "answers.json", {"*": plan_answers(comp_p=0.62)})
            j = Judged(d, Transport(), env_values={}, extra_args=["--answers-file", fixture])
            self.assertEqual((j.result["verdict"], j.result["attempts"]), ("satisfied", 1))
            self.assertNotIn("near_threshold", j.result)
        with tempfile.TemporaryDirectory() as d:
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.62)}, http(503), http(503), http(503))
            j = Judged(d, t)
        self.assertEqual((j.code, j.result["verdict"], j.result["attempts"], j.result["confirm_error"]["code"]), (0, "satisfied", 4, "JEV.HTTP"))
        self.assertEqual(j.sleeps, [1.0, 2.0])
        with tempfile.TemporaryDirectory() as d:  # the second call hitting 401 still aborts the run
            t = Transport({"model": "jev", "answers": plan_answers(comp_p=0.62)}, http(401))
            j = Judged(d, t)
        self.assertEqual((j.code, j.out["error"]["code"]), (2, "AUTH.CREDENTIALS"))


class RequestBytesDefaultTests(unittest.TestCase):
    def test_default_depends_on_the_gate_and_the_flag_overrides(self):
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(), extra_args=["--dry-run"])
            self.assertEqual((j.run["max_request_bytes"], j.run["totals"]["max_request_bytes"]), (24000, 24000))
            j = Judged(d, Transport(gate="dev"), gate="dev", extra_args=["--dry-run"])
            self.assertEqual(j.run["max_request_bytes"], 64000)
            j = Judged(d, Transport(gate="dev"), gate="dev", extra_args=["--dry-run", "--max-request-bytes", "9000"])
            self.assertEqual(j.run["max_request_bytes"], 9000)
            # the dev default lets a 40k item through in one attempt; the plan default refuses it
            t = Transport(gate="dev")
            j = Judged(d, t, gate="dev", item_text="e" * 40000)
            self.assertEqual((j.result["attempts"], j.result["item_shortened"]), (1, False))
            j = Judged(d, Transport(), item_text="e" * 23000)
            self.assertEqual((j.code, j.out["error"]["code"]), (2, "PLAN.TOO_LARGE"))
        self.assertEqual((jj.DEFAULT_MAX_REQUEST_BYTES, jj.DEFAULT_MAX_REQUEST_BYTES_DEV), (24000, 64000))


class ValidateTests(unittest.TestCase):
    def test_problems_are_listed(self):
        rec = record(
            anchor("ANC-1", status="locked"),
            anchor("ANC-002", status="bogus", statement="  "),
            anchor("ANC-002"),
            anchor("REQ-014", source_type="REQ", source_ref=None),
            anchor("ANC-003", status="promoted"),
            anchor("ANC-004", source_type="XYZ", stated_at="", question_overrides={"prod": {}}),
            "not an object",
        )
        rec["schema"] = "wrong"
        problems = jj.validate_record(rec)
        joined = "\n".join(problems)
        for needle in ("schema must be", "ANC-1: ANC ids look like", "ANC-002: status must be", "ANC-002: statement is empty",
                       "ANC-002: duplicate id", "REQ-014: REQ/ADR anchors need source_ref", "ANC-003: promoted anchors need promoted_to",
                       "ANC-004: source_type must be", "ANC-004: stated_at missing", "question_overrides key must be plan or dev",
                       "anchors[6]: must be an object"):
            self.assertIn(needle, joined)
        self.assertEqual(jj.validate_record(record(anchor())), [])
        self.assertEqual(jj.validate_record({"schema": "plan-anchoring/anchors/1", "anchors": {}}), ["'anchors' must be a list"])

    def test_validate_cli_exit_codes_and_warnings(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(status="proposed")))
            code, out = run_cli(["validate", "--anchors", path])
            self.assertEqual(code, 0)
            self.assertIn("no locked anchors", out["data"]["warnings"])
            self.assertIn("intent_ref file not found: doc/plan/phase-current/intent.md", out["data"]["warnings"])
            Path(d, "intent.md").write_text("# intent\n")
            code, out = run_cli(["validate", "--anchors", path])
            self.assertEqual([w for w in out["data"]["warnings"] if "intent_ref" in w], [])
            path = write(d, "bad.json", record(anchor(status="nope")))
            code, out = run_cli(["validate", "--anchors", path])
            self.assertEqual((code, out["success"], out["error"]["code"]), (1, False, "VALIDATION.ANCHORS"))
            self.assertTrue(out["data"]["problems"])


class DecideTests(unittest.TestCase):
    def decide(self, answers, roles=PLAN_ROLES, t=0.6):
        return jj.decide(answers, roles, t)

    def test_row1_not_applicable(self):
        self.assertEqual(self.decide(plan_answers(rel="no", comp="no", conf="no"))[0], "not_applicable")
        # relevance "no" below threshold falls through to underspecified
        self.assertEqual(self.decide(plan_answers(rel="no", rel_p=0.5))[0], "underspecified")

    def test_row2_contradicted_outranks_underspecification(self):
        self.assertEqual(self.decide(plan_answers(rel="yes", comp="no", conf="yes"))[0], "contradicted")
        self.assertEqual(self.decide(plan_answers(rel="unclear", comp="unclear", conf="yes"))[0], "contradicted")
        self.assertEqual(self.decide(plan_answers(rel="yes", comp="no", conf="yes", conf_p=0.55))[0], "not_addressed")

    def test_row3_underspecified(self):
        self.assertEqual(self.decide(plan_answers(rel="unclear"))[0], "underspecified")
        self.assertEqual(self.decide(plan_answers(rel_p=0.59))[0], "underspecified")
        self.assertEqual(self.decide(plan_answers(comp="unclear"))[0], "underspecified")
        # a low-confidence conflict yes (or unclear) never passes, even when compliance is a confident yes
        self.assertEqual(self.decide(plan_answers(conf="yes", conf_p=0.4)), ("underspecified", 0.4))
        self.assertEqual(self.decide(plan_answers(conf="unclear"))[0], "underspecified")
        self.assertEqual(self.decide(plan_answers(conf="no", conf_p=0.25))[0], "underspecified")
        verdict, min_conf = self.decide(plan_answers(comp_p=0.3))
        self.assertEqual((verdict, min_conf), ("underspecified", 0.3))

    def test_row4_not_addressed(self):
        self.assertEqual(self.decide(plan_answers(comp="no"))[0], "not_addressed")

    def test_row5_asserted_only_needs_evidence_role(self):
        self.assertEqual(self.decide(dev_answers(evid="no"), DEV_ROLES)[0], "asserted_only")
        self.assertEqual(self.decide(dev_answers(evid="no", evid_p=0.4), DEV_ROLES)[0], "satisfied")
        self.assertEqual(self.decide(dev_answers(evid="unclear"), DEV_ROLES)[0], "satisfied")
        no_evidence_role = {k: v for k, v in DEV_ROLES.items() if k != "evidence"}
        self.assertEqual(self.decide(dev_answers(evid="no"), no_evidence_role)[0], "satisfied")

    def test_row6_satisfied_and_min_confidence(self):
        verdict, min_conf = self.decide(plan_answers(rel_p=0.95, comp_p=0.7, conf_p=0.8))
        self.assertEqual((verdict, min_conf), ("satisfied", 0.7))

    def test_row7_fallthrough(self):
        answers = plan_answers()
        answers["addressed"]["choice"] = "maybe"
        self.assertEqual(self.decide(answers)[0], "underspecified")
        self.assertEqual(self.decide({}), ("underspecified", 0.0))

    def test_threshold_boundaries(self):
        self.assertEqual(self.decide(plan_answers(p=0.6), t=0.6)[0], "satisfied")
        self.assertEqual(self.decide(plan_answers(p=0.6), t=0.61)[0], "underspecified")
        self.assertEqual(self.decide(plan_answers(rel="no", p=0.1), t=0.0)[0], "not_applicable")

    def test_noul_fallback(self):
        self.assertEqual(jj.answer_choice({"type": "noul", "noul": 0.8}), ("yes", 0.6000000000000001))
        self.assertEqual(jj.answer_choice({"type": "noul", "probability": 0.2}), ("no", 0.6))
        self.assertEqual(jj.answer_choice({"noul": 0.5}), ("yes", 0.0))
        answers = plan_answers()
        answers["contradicted"] = {"type": "noul", "noul": 0.9}
        self.assertEqual(self.decide(answers)[0], "contradicted")
        self.assertIsNone(jj.answer_problem({"type": "noul", "noul": 0.3}, {"criteria": {}}))
        self.assertIsNotNone(jj.answer_problem({"type": "noul"}, {"criteria": {}}))


class ResponseValidationTests(unittest.TestCase):
    question = {"type": "choice", "criteria": {"yes": "", "no": "", "unclear": ""}}

    def test_good_answer_passes_and_rounding_is_tolerated(self):
        self.assertIsNone(jj.answer_problem(answer("yes"), self.question))
        ok = answer("yes")
        ok["probabilities"] = {"yes": 0.94, "no": 0.05, "unclear": 0.0}
        self.assertIsNone(jj.answer_problem(ok, self.question))

    def test_problems_named(self):
        bad = answer("yes")
        bad["probabilities"]["yes"] = 0.5
        self.assertRegex(jj.answer_problem(bad, self.question), "sum to")
        self.assertRegex(jj.answer_problem(answer("maybe"), self.question), "not one of")
        self.assertRegex(jj.answer_problem({"type": "choice", "choice": "yes", "confidence": "0.9", "probabilities": {}}, self.question), "confidence")
        self.assertRegex(jj.answer_problem({"type": "choice", "choice": "yes", "confidence": True, "probabilities": {}}, self.question), "confidence")
        extra = answer("yes")
        extra["probabilities"]["other"] = 0.0
        self.assertRegex(jj.answer_problem(extra, self.question), "keys differ")
        self.assertRegex(jj.answer_problem({"type": "score", "choice": "yes"}, self.question), "not choice or noul")
        self.assertRegex(jj.answer_problem("yes", self.question), "not an object")

    def test_validate_response_requires_exact_answer_ids(self):
        questions = {"relevant": self.question, "addressed": self.question, "contradicted": self.question}
        with self.assertRaises(jj.JudgeError) as cm:
            jj.validate_response({"answers": {"relevant": answer("yes")}}, questions)
        self.assertEqual(cm.exception.code, "JEV.RESPONSE_INVALID")
        self.assertTrue(cm.exception.recoverable)
        extra = dict(plan_answers(), bonus=answer("yes"))
        with self.assertRaises(jj.JudgeError):
            jj.validate_response({"answers": extra}, questions)
        with self.assertRaises(jj.JudgeError):
            jj.validate_response({"answers": []}, questions)
        with self.assertRaises(jj.JudgeError):
            jj.validate_response(["not", "a", "dict"], questions)
        self.assertIs(jj.validate_response({"model": "x", "answers": plan_answers()}, questions)["answers"]["relevant"]["choice"], "yes")

    def test_shipped_fixture_passes_validation(self):
        fixture = json.loads((Path(__file__).resolve().parents[1] / "assets" / "examples" / "answers.fixture.json").read_text())
        plan = jj.load_question_set("plan")
        for aid, answers in fixture.items():
            jj.validate_response({"answers": answers}, plan["questions"])


class HttpErrorTests(unittest.TestCase):
    def test_bodies_withheld_except_422(self):
        e401 = http(401, "secret body")
        self.assertEqual(e401.code, "AUTH.CREDENTIALS")
        self.assertNotIn("secret", str(e401))
        e422 = http(422, "x" * 400)
        self.assertEqual(e422.code, "JEV.VALIDATION")
        self.assertIn("x" * 300, str(e422))
        self.assertNotIn("x" * 301, str(e422))
        e500 = http(500, "stack trace")
        self.assertEqual((e500.code, e500.recoverable, str(e500)), ("JEV.HTTP", True, "Jev HTTP 500; response body withheld"))
        self.assertEqual((http(429).code, http(529).code, http(418).recoverable), ("JEV.RATE_LIMITED", "JEV.OVERLOADED", False))
        self.assertTrue(jj.is_transient(e500))
        self.assertFalse(jj.is_transient(http(418)))

    def test_context_overflow_detected_by_body(self):
        for body in ('{"error": "max_tokens_exceeded"}', "Token budget exceeded for state", "The TOKEN count EXCEEDS the limit"):
            self.assertEqual(http(422, body).code, "JEV.CONTEXT_EXCEEDED", body)
            self.assertEqual(http(400, body).code, "JEV.CONTEXT_EXCEEDED", body)
        self.assertEqual(http(500, "max_tokens_exceeded").code, "JEV.HTTP")
        self.assertEqual(http(422, "field 'questions.addressed.criteria' is invalid").code, "JEV.VALIDATION")

    def test_retry_after_parsing(self):
        self.assertEqual(jj.parse_retry_after("3"), 3.0)
        self.assertEqual(jj.parse_retry_after("900"), jj.MAX_RETRY_AFTER)
        self.assertIsNone(jj.parse_retry_after("Wed, 21 Oct 2026"))
        self.assertIsNone(jj.parse_retry_after(None))
        self.assertIsNone(jj.parse_retry_after("-1"))


class OverrideTests(unittest.TestCase):
    def test_merge_replaces_by_id_and_roles(self):
        base = jj.load_question_set("plan")
        merged = jj.merge_overrides(base, {"questions": {"addressed": {"type": "choice", "instructions": "override", "criteria": {}}}})
        self.assertEqual(merged["questions"]["addressed"]["instructions"], "override")
        self.assertEqual(merged["questions"]["relevant"], base["questions"]["relevant"])
        self.assertEqual(merged["roles"], base["roles"])
        self.assertIs(jj.merge_overrides(base, {})["questions"] is base["questions"], False)
        renamed = jj.merge_overrides(base, {"questions": {"covers": base["questions"]["addressed"]}, "roles": {"compliance": "covers"}})
        self.assertEqual(renamed["roles"]["compliance"], "covers")
        jj.check_roles(renamed, "t")

    def test_malformed_overrides_are_refused(self):
        base = jj.load_question_set("plan")
        for bad in (["list"], {"questions": ["list"]}, {"roles": "compliance=x"}):
            with self.assertRaises(jj.JudgeError) as cm:
                jj.merge_overrides(base, bad)
            self.assertEqual(cm.exception.code, "INPUT.MALFORMED")

    def test_roles_rechecked_after_merge(self):
        base = jj.load_question_set("plan")
        bad = jj.merge_overrides(base, {"roles": {"compliance": "covers"}})
        with self.assertRaises(jj.JudgeError) as cm:
            jj.check_roles(bad, "t")
        self.assertEqual(cm.exception.code, "QUESTIONS.ROLES")
        dev = jj.load_question_set("dev")
        with self.assertRaises(jj.JudgeError) as cm:
            jj.check_roles(jj.merge_overrides(dev, {"roles": {"evidence": "missing"}}), "t")
        self.assertEqual(cm.exception.code, "QUESTIONS.ROLES")
        with self.assertRaises(jj.JudgeError) as cm:
            jj.check_roles({"roles": PLAN_ROLES, "questions": {}}, "t")
        self.assertEqual(cm.exception.code, "INPUT.MALFORMED")

    def test_precedence_record_then_overrides_file(self):
        base = jj.load_question_set("plan")
        with tempfile.TemporaryDirectory() as d:
            a = anchor(question_overrides={"plan": {"questions": {"addressed": {"type": "choice", "instructions": "from record", "criteria": {"yes": "", "no": "", "unclear": ""}}}}})
            anchors_path = write(d, "anchors.json", record(a))
            fixture_path = write(d, "answers.json", {"*": plan_answers()})
            item_path = write(d, "item.txt", "Sprint 2 copies the legacy crates.")
            common = ["judge", "--anchors", anchors_path, "--gate", "plan", "--all", "--item-id", "bd-42", "--item-file", item_path,
                      "--answers-file", fixture_path, "--dry-run"]
            code, out = run_cli(common + ["--out", str(Path(d) / "r1.json")])
            self.assertEqual(out["data"]["requests"][0]["questions"]["addressed"]["instructions"], "from record")
            entry = out["data"]["results"][0]
            self.assertTrue(entry["question_set"]["overridden"])
            self.assertEqual(entry["questions"]["addressed"]["instructions"], "from record")
            override_path = write(d, "overrides.json", {"ANC-001": {"questions": {"addressed": {"type": "choice", "instructions": "from bead", "criteria": {"yes": "", "no": "", "unclear": ""}}}}})
            code, out2 = run_cli(common + ["--overrides-file", override_path, "--out", str(Path(d) / "r2.json")])
            self.assertEqual(out2["data"]["requests"][0]["questions"]["addressed"]["instructions"], "from bead")
            self.assertNotEqual(entry["questions_sha256"], out2["data"]["results"][0]["questions_sha256"])
            self.assertEqual(entry["questions_sha256"], jj.questions_digest(jj.merge_overrides(base, a["question_overrides"]["plan"])["questions"]))
            # a bead override that renames a question without updating roles aborts before anything is sent
            bad_path = write(d, "bad.json", {"ANC-001": {"roles": {"compliance": "nope"}}})
            code, out3 = run_cli(common + ["--overrides-file", bad_path])
            self.assertEqual((code, out3["error"]["code"]), (2, "QUESTIONS.ROLES"))

    def test_unoverridden_entries_carry_only_the_digest(self):
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport())
            self.assertFalse(j.result["question_set"]["overridden"])
            self.assertNotIn("questions", j.result)
            self.assertEqual(len(j.result["questions_sha256"]), 64)


class JudgeFixtureTests(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.TemporaryDirectory()
        self.anchors = write(self.d.name, "anchors.json", record(anchor(), anchor("REQ-014", source_type="REQ", source_ref="REQ-014", statement="All public CLI commands must support --json output."), anchor("ANC-002", status="proposed")))
        self.item = write(self.d.name, "item.txt", "Sprint 2 copies the legacy runtime crates into crates/runtime.")
        self.run_dir = str(Path(self.d.name) / "runs")

    def tearDown(self):
        self.d.cleanup()

    def judge(self, *extra):
        with env():
            return run_cli(["judge", "--anchors", self.anchors, "--gate", "plan", "--item-id", "bd-42", "--item-file", self.item,
                            "--run-dir", self.run_dir, *extra])

    def test_fixture_answers_by_id_and_star(self):
        fixture = write(self.d.name, "answers.json", {"ANC-001": plan_answers(comp="no"), "*": plan_answers(rel="no")})
        code, out = self.judge("--all", "--answers-file", fixture)
        run = out["data"]
        self.assertEqual(code, 1)
        verdicts = {r["anchor_id"]: r["verdict"] for r in run["results"]}
        self.assertEqual(verdicts, {"ANC-001": "not_addressed", "REQ-014": "not_applicable"})
        self.assertEqual(run["results"][0]["anchor_id"], "ANC-001")  # failing first
        self.assertEqual(run["summary"], {"counts": {"not_addressed": 1, "not_applicable": 1}, "failing": ["ANC-001"],
                                          "leaning_counts": {"not_addressed": 1, "not_applicable": 1}, "unstable": [], "errors": [], "shortened": [], "pass": False})
        self.assertTrue(run["fixture"])
        self.assertEqual(run["model_answered"], "fixture")
        self.assertEqual(run["results"][0]["attempts"], 1)
        self.assertEqual(run["path"], run["run_path"])
        self.assertTrue(Path(run["path"]).is_file())
        self.assertTrue(Path(run["path"]).name.startswith("plan-") and Path(run["path"]).name.endswith("-bd-42.json"))

    def test_passing_run_exits_zero(self):
        fixture = write(self.d.name, "answers.json", {"*": plan_answers()})
        code, out = self.judge("--anchor", "ANC-001", "--answers-file", fixture)
        self.assertEqual((code, out["data"]["summary"]["pass"]), (0, True))

    def test_missing_fixture_entry_is_an_error_verdict(self):
        fixture = write(self.d.name, "answers.json", {"REQ-014": plan_answers()})
        code, out = self.judge("--all", "--answers-file", fixture)
        by_id = {r["anchor_id"]: r for r in out["data"]["results"]}
        self.assertEqual(by_id["ANC-001"]["verdict"], "error")
        self.assertEqual(by_id["ANC-001"]["error"]["code"], "FIXTURE.MISSING")
        self.assertEqual(by_id["REQ-014"]["verdict"], "satisfied")
        self.assertEqual((code, out["data"]["summary"]["errors"]), (1, ["ANC-001"]))

    def test_malformed_fixture_answer_is_response_invalid(self):
        bad = plan_answers()
        bad["relevant"]["probabilities"]["yes"] = 0.1
        fixture = write(self.d.name, "answers.json", {"*": bad})
        code, out = self.judge("--anchor", "ANC-001", "--answers-file", fixture)
        self.assertEqual(out["data"]["results"][0]["error"]["code"], "JEV.RESPONSE_INVALID")

    def test_explicit_anchor_must_be_locked_unless_included(self):
        fixture = write(self.d.name, "answers.json", {"*": plan_answers()})
        code, out = self.judge("--anchor", "ANC-002", "--answers-file", fixture)
        self.assertEqual((code, out["error"]["code"]), (2, "ANCHOR.NOT_LOCKED"))
        code, out = self.judge("--anchor", "ANC-002", "--anchor", "ANC-002", "--include-unlocked", "--answers-file", fixture)
        self.assertEqual([r["anchor_id"] for r in out["data"]["results"]], ["ANC-002"])  # de-duplicated
        code, out = self.judge("--anchor", "ANC-999", "--answers-file", fixture)
        self.assertEqual(out["error"]["code"], "ANCHOR.UNKNOWN")

    def test_all_skips_proposed_and_requires_some_anchor(self):
        fixture = write(self.d.name, "answers.json", {"*": plan_answers()})
        code, out = self.judge("--all", "--answers-file", fixture)
        self.assertEqual(sorted(r["anchor_id"] for r in out["data"]["results"]), ["ANC-001", "REQ-014"])
        code, out = self.judge("--answers-file", fixture)
        self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")
        only_proposed = write(self.d.name, "p.json", record(anchor(status="proposed")))
        with env():
            code, out = run_cli(["judge", "--anchors", only_proposed, "--gate", "plan", "--all", "--item-id", "x", "--item-file", self.item, "--answers-file", fixture])
        self.assertEqual(out["error"]["code"], "ANCHOR.NONE")

    def test_dry_run_writes_requests_and_totals_and_exits_zero(self):
        code, out = self.judge("--all", "--dry-run")
        run = out["data"]
        self.assertEqual(code, 0)
        self.assertTrue(run["dry_run"])
        self.assertFalse(run["summary"]["pass"])
        self.assertEqual(len(run["requests"]), 2)
        self.assertEqual(run["requests"][0]["state"]["item"]["id"], "bd-42")
        self.assertEqual(set(run["requests"][0]["questions"]), {"relevant", "addressed", "contradicted"})
        self.assertEqual(run["totals"]["requests"], 2)
        self.assertEqual(run["totals"]["bytes"], sum(len(jj.encode_body(b)) for b in run["requests"]))
        self.assertEqual(run["totals"]["max_request_bytes"], 24000)
        self.assertTrue(Path(run["run_path"]).is_file() and Path(run["path"]).parent == Path(self.run_dir))
        self.assertIsNone(run["results"][0]["verdict"])
        self.assertEqual(run["results"][0]["attempts"], 0)
        with self.assertRaises(jj.JudgeError):
            jj.cmd_coverage(jj.argparse.Namespace(runs=[self.run_dir], gate="", format="json"))  # dry runs never count

    def test_judge_refuses_invalid_record_and_options(self):
        bad = write(self.d.name, "bad.json", record(anchor(status="nope")))
        fixture = write(self.d.name, "answers.json", {"*": plan_answers()})
        with env():
            code, out = run_cli(["judge", "--anchors", bad, "--gate", "plan", "--all", "--item-id", "x", "--item-file", self.item, "--answers-file", fixture])
        self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.ANCHORS"))
        code, out = self.judge("--all", "--answers-file", fixture, "--threshold", "1.5")
        self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")
        code, out = self.judge("--all", "--answers-file", fixture, "--min-item-chars", "0")
        self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")

    def test_read_item_truncates_and_rejects_empty(self):
        long_item = write(self.d.name, "long.txt", "x" * 100)
        ns = jj.argparse.Namespace(item_file=long_item, item_id="bd-1", item_title="", max_item_chars=40)
        item = jj.read_item(ns)
        self.assertTrue(item["truncated"])
        self.assertTrue(item["text"].startswith("x" * 40) and item["text"].endswith(jj.TRUNCATION_MARKER))
        self.assertEqual(jj.item_core(item["text"]), "x" * 40)
        empty = write(self.d.name, "empty.txt", "  \n")
        with self.assertRaises(jj.JudgeError) as cm:
            jj.read_item(jj.argparse.Namespace(item_file=empty, item_id="x", item_title="", max_item_chars=10))
        self.assertEqual(cm.exception.code, "INPUT.EMPTY")
        with self.assertRaises(jj.JudgeError) as cm:
            jj.read_item(jj.argparse.Namespace(item_file=str(Path(self.d.name) / "nope.txt"), item_id="x", item_title="", max_item_chars=10))
        self.assertEqual(cm.exception.code, "INPUT.MISSING")

    def test_run_file_names_are_sanitised_and_never_collide(self):
        with env():
            run_a = jj.assemble_run({"gate": "plan", "model": "m", "threshold": 0.6, "max_request_bytes": 1, "min_item_chars": 1, "dry_run": False, "answers_fixture": None},
                                    record(), "a.json", {"id": "PR #12/x", "title": "", "text": "t", "truncated": False, "source": "s"}, [], [], None)
        self.assertTrue(run_a["run_id"].endswith("-PR__12_x"))
        run_dir = Path(self.d.name) / "coll"
        p1 = jj.write_run(dict(run_a), "", run_dir)
        run_b = dict(run_a)
        p2 = jj.write_run(run_b, "", run_dir)
        self.assertNotEqual(p1, p2)
        self.assertTrue(run_b["run_id"].endswith("-2") and p2.endswith("-2.json"))
        self.assertEqual(sorted(x.name for x in run_dir.iterdir() if x.suffix == ".tmp"), [])


class FailClosedTests(unittest.TestCase):
    def test_missing_key_and_missing_opt_in(self):
        with tempfile.TemporaryDirectory() as d:
            transport = Transport()
            j = Judged(d, transport, env_values={})
            self.assertEqual((j.code, j.out["error"]["code"]), (2, "AUTH.CREDENTIALS"))
            j = Judged(d, transport, env_values={"TYPESAFE_API_KEY": "sk-test"})
            self.assertEqual((j.code, j.out["error"]["code"]), (2, "POLICY.REMOTE_JUDGE_DISABLED"))
            self.assertEqual(transport.bodies, [])  # nothing left the machine
            j = Judged(d, transport, env_values={"TYPESAFE_API_KEY": "sk-test"}, extra_args=["--allow-remote"])
            self.assertEqual((j.code, j.result["verdict"]), (0, "satisfied"))
            self.assertEqual(len(transport.bodies), 1)

    def test_key_format_checked_and_never_echoed(self):
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(), env_values={"TYPESAFE_API_KEY": "sk-bad\nkey", "PLAN_ANCHOR_ALLOW_REMOTE_JUDGE": "1"})
            self.assertEqual((j.code, j.out["error"]["code"]), (2, "AUTH.CREDENTIALS"))
            self.assertNotIn("sk-bad", json.dumps(j.out))
            j = Judged(d, Transport(), env_values={"TYPESAFE_API_KEY": "  sk-ok  ", "PLAN_ANCHOR_ALLOW_REMOTE_JUDGE": "1"})
            self.assertEqual(j.code, 0)

    def test_fixture_and_dry_run_need_neither(self):
        with tempfile.TemporaryDirectory() as d:
            fixture = write(d, "answers.json", {"*": plan_answers()})
            j = Judged(d, Transport(), env_values={}, extra_args=["--answers-file", fixture])
            self.assertEqual(j.code, 0)
            j = Judged(d, Transport(), env_values={}, extra_args=["--dry-run"])
            self.assertEqual(j.code, 0)

    def test_every_emitted_value_redacts_the_live_key_and_bearer_tokens(self):
        buf = io.StringIO()
        with env(TYPESAFE_API_KEY="ts-secret-123456"), contextlib.redirect_stdout(buf):
            jj.emit({"message": "key=ts-secret-123456", "header": "Bearer another-secret"})
        rendered = buf.getvalue()
        self.assertNotIn("ts-secret-123456", rendered)
        self.assertNotIn("another-secret", rendered)
        self.assertEqual(json.loads(rendered), {"message": "key=<redacted>", "header": "Bearer <redacted>"})


class AgentPreflightTests(unittest.TestCase):
    def test_normalises_workspace_paths_and_identifiers(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d) / "repo"
            root.mkdir()
            anchors = write(root, "doc/plan/phase-current/anchors.json", record(anchor()))
            evidence = write(root, "reports/sprint.md", "done")
            code, out = run_cli([
                "agent-preflight", "--workspace-root", str(root), "--script", jj.__file__,
                "--anchors", "doc/plan/phase-current/anchors.json", "--run-dir", "doc/plan/phase-current/runs",
                "--evidence", evidence, "--identifier", "proj-s2", "--pr-number", "12",
                "--correlation-id", "proj-s2:dev",
            ])
            self.assertEqual(code, 0)
            self.assertEqual(out["data"]["anchors_path"], str(Path(anchors).resolve()))
            self.assertEqual(out["data"]["evidence"], [str(Path(evidence).resolve())])
            self.assertEqual(out["data"]["work_key"], "proj-s2_dev")

    def test_rejects_paths_outside_the_workspace(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d) / "repo"
            root.mkdir()
            anchors = write(root, "anchors.json", record(anchor()))
            outside = write(d, "secret.txt", "do not upload")
            code, out = run_cli([
                "agent-preflight", "--workspace-root", str(root), "--script", jj.__file__,
                "--anchors", anchors, "--evidence", outside,
            ])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.PATH"))
            self.assertIn("outside the repository", out["error"]["message"])

    def test_rejects_unsafe_ids_pr_numbers_and_script_mismatch(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            anchors = write(root, "anchors.json", record(anchor()))
            base = ["agent-preflight", "--workspace-root", str(root), "--script", jj.__file__, "--anchors", anchors]
            code, out = run_cli([*base, "--identifier", "../../secret"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.ID"))
            code, out = run_cli([*base, "--pr-number", "12oops"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.ID"))
            other = write(root, "other.py", "pass")
            code, out = run_cli([
                "agent-preflight", "--workspace-root", str(root), "--script", other, "--anchors", anchors,
            ])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.PATH"))
            code, out = run_cli([
                "agent-preflight", "--workspace-root", str(root), "--script", str(root / "missing.py"),
                "--anchors", anchors,
            ])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.PATH"))
            self.assertIn("script could not be resolved", out["error"]["message"])
            code, out = run_cli([*base, "--run-dir", write(root, "not-a-directory", "x")])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.PATH"))
            self.assertIn("run directory is not a directory", out["error"]["message"])


class TransportPolicyTests(unittest.TestCase):
    """Shortening (truncate, halve on overflow, pre-flight cap) is development-gate behaviour; the plan gate refuses instead (PlanSizeTests)."""

    def test_overflow_once_then_success_halves_the_item(self):
        text = "y" * 6000
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(422, "max_tokens_exceeded"), gate="dev")
            j = Judged(d, t, item_text=text, gate="dev")
        self.assertEqual((j.code, j.result["verdict"], j.result["attempts"]), (0, "satisfied", 2))
        self.assertTrue(j.result["item_shortened"])
        self.assertEqual(j.result["item_chars_sent"], 3000 + len(jj.TRUNCATION_MARKER))
        self.assertEqual(j.sleeps, [])
        sent = t.bodies[1]["state"]["item"]["text"]
        self.assertTrue(sent.startswith("y" * 3000) and sent.endswith(jj.TRUNCATION_MARKER))
        self.assertEqual(t.bodies[0]["state"]["item"]["text"], text)
        self.assertEqual(j.run["summary"]["shortened"], ["ANC-001"])
        self.assertEqual(j.run["item"]["chars"], 6000)  # the run records what the item was, the entry what was sent

    def test_overflow_down_to_the_floor_records_context_exceeded(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(*[http(422, "max_tokens_exceeded")] * 10, gate="dev")
            j = Judged(d, t, item_text="y" * 5000, extra_args=["--min-item-chars", "2000"], gate="dev")
        self.assertEqual((j.code, j.result["verdict"]), (1, "error"))
        self.assertEqual(j.result["error"]["code"], "JEV.CONTEXT_EXCEEDED")
        self.assertIn("Shorten the evidence", j.result["error"]["suggested_action"])
        self.assertEqual(j.result["attempts"], 3)  # 5000, 2500, 2000 (floor), then give up
        self.assertEqual([len(jj.item_core(b["state"]["item"]["text"])) for b in t.bodies], [5000, 2500, 2000])
        self.assertTrue(j.result["item_shortened"])
        self.assertEqual(j.result["item_chars_sent"], 2000 + len(jj.TRUNCATION_MARKER))
        self.assertTrue(j.out["success"])  # the run envelope stays success; the anchor carries the error

    def test_preflight_cap_shortens_before_the_first_call(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(gate="dev")
            j = Judged(d, t, item_text="z" * 20000, extra_args=["--max-request-bytes", "8000"], gate="dev")
        self.assertEqual((j.code, j.result["attempts"], j.result["item_shortened"]), (0, 1, True))
        self.assertLessEqual(len(jj.encode_body(t.bodies[0])), 8000)
        self.assertLessEqual(j.result["request_bytes"], 8000)
        self.assertLess(j.result["item_chars_sent"], 20000)

    def test_preflight_cap_impossible_makes_no_call(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(gate="dev")
            j = Judged(d, t, item_text="z" * 3000, extra_args=["--max-request-bytes", "500"], gate="dev")
        self.assertEqual((j.result["verdict"], j.result["error"]["code"], j.result["attempts"]), ("error", "JEV.CONTEXT_EXCEEDED", 0))
        self.assertEqual(t.bodies, [])
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(gate="dev"), item_text="z" * 3000, extra_args=["--max-request-bytes", "500", "--dry-run"], gate="dev")
        self.assertEqual(j.result["error"]["code"], "JEV.CONTEXT_EXCEEDED")
        self.assertEqual(j.code, 0)

    def test_rate_limited_twice_then_success(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(429, retry_after=3.5), http(429))
            j = Judged(d, t)
        self.assertEqual((j.code, j.result["verdict"], j.result["attempts"]), (0, "satisfied", 3))
        self.assertEqual(j.sleeps, [3.5, 2.0])
        self.assertEqual(len(t.bodies), 3)
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(http(529), jj.JudgeError("JEV.NETWORK", "down", recoverable=True)))
        self.assertEqual((j.result["attempts"], j.sleeps), (3, [1.0, 2.0]))

    def test_three_failures_record_an_error_verdict(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(529), http(529), http(529), http(529))
            j = Judged(d, t)
        self.assertEqual((j.code, j.result["verdict"], j.result["attempts"]), (1, "error", 3))
        self.assertEqual(j.result["error"]["code"], "JEV.OVERLOADED")
        self.assertTrue(j.result["error"]["recoverable"])
        self.assertEqual(len(t.bodies), 3)
        self.assertEqual(j.sleeps, [1.0, 2.0])
        self.assertEqual(j.run["summary"]["errors"], ["ANC-001"])
        self.assertFalse(j.run["summary"]["pass"])
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(http(503), http(503), http(503)))
        self.assertEqual((j.result["verdict"], j.result["error"]["code"], j.result["attempts"]), ("error", "JEV.HTTP", 3))

    def test_invalid_response_is_retried_like_a_transport_failure(self):
        good = {"model": "jev-1.13.0", "answers": plan_answers()}
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport({"answers": {}}, {"answers": plan_answers() | {"extra": answer("yes")}}, good))
        self.assertEqual((j.result["verdict"], j.result["attempts"], j.sleeps), ("satisfied", 3, [1.0, 2.0]))
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport({"answers": {}}, {"answers": {}}, {"answers": {}}))
        self.assertEqual((j.result["verdict"], j.result["error"]["code"], j.result["attempts"]), ("error", "JEV.RESPONSE_INVALID", 3))

    def test_credential_and_validation_errors_are_not_retried(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(401))
            j = Judged(d, t, anchors=[anchor(), anchor("ANC-002")])
        self.assertEqual((j.code, j.out["error"]["code"], len(t.bodies)), (2, "AUTH.CREDENTIALS", 1))
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(422, "questions.addressed.criteria must be an object"))
            j = Judged(d, t)
        self.assertEqual((j.result["verdict"], j.result["error"]["code"], j.result["attempts"], j.sleeps), ("error", "JEV.VALIDATION", 1, []))
        self.assertIn("questions.addressed.criteria", j.result["error"]["message"])

    def test_one_anchor_failing_does_not_lose_the_others(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(529), http(529), http(529))  # first anchor exhausts; second anchor gets the default
            j = Judged(d, t, anchors=[anchor(), anchor("ANC-002")])
        verdicts = {r["anchor_id"]: r["verdict"] for r in j.run["results"]}
        self.assertEqual(verdicts, {"ANC-001": "error", "ANC-002": "satisfied"})
        self.assertEqual([r["verdict"] for r in j.run["results"]], ["satisfied", "error"])  # severity order: error sorts last

    def test_model_answered_is_recorded(self):
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport({"model": "jev-1.13.0", "answers": plan_answers(), "usage": {"input_tokens": 5}}))
        self.assertEqual((j.run["model_answered"], j.run["model_requested"], j.result["usage"]), ("jev-1.13.0", jj.DEFAULT_MODEL, {"input_tokens": 5}))


RUST_BLOCK = """```rust
pub struct StepSelector {
    pub name: String,
    pub groups: Vec<String>,
}

impl StepSelector {
    pub fn matches(
        &self,
        step: &Step,
    ) -> bool {
        self.name == step.name
            || self.groups.iter().any(|g| step.groups.contains(g))
    }
}
```"""


class PlanSizeTests(unittest.TestCase):
    """Ported from the plan-size fork branch: the plan gate reduces code and refuses, never shortens."""

    def test_long_code_block_is_reduced_to_signatures_with_a_marker(self):
        text, stats = jj.reduce_code("Design:\n\n" + RUST_BLOCK + "\n\nDone.", 8)
        self.assertIn("```rust\npub struct StepSelector {\nimpl StepSelector {\n    pub fn matches(\n        &self,\n        step: &Step,\n    ) -> bool {\n", text)
        self.assertIn("<redacted: 8 of 14 code lines are in the plan but were not sent to Jev>\n```", text)
        self.assertNotIn("self.groups.iter()", text)
        self.assertTrue(text.startswith("Design:") and text.endswith("Done."))
        self.assertEqual(stats, {"blocks": 1, "lines": 8})

    def test_short_blocks_and_prose_are_untouched(self):
        short = 'Intro\n```toml\n[lint]\nprofile = "ci"\n```\nOutro'
        self.assertEqual(jj.reduce_code(short, 8), (short, {"blocks": 0, "lines": 0}))
        self.assertEqual(jj.reduce_code(RUST_BLOCK, 15)[1], {"blocks": 0, "lines": 0})

    def test_block_without_signatures_keeps_only_the_marker(self):
        block = "```text\n" + "\n".join(f"line {n}" for n in range(12)) + "\n```"
        text, stats = jj.reduce_code(block, 8)
        self.assertEqual(text, "```text\n<redacted: 12 of 12 code lines are in the plan but were not sent to Jev>\n```")
        self.assertEqual(stats, {"blocks": 1, "lines": 12})

    def test_unterminated_fence_is_reduced_and_closed(self):
        text, stats = jj.reduce_code("```rust\n" + "fn a() {}\n" + "x();\n" * 10, 8)
        self.assertTrue(text.startswith("```rust\nfn a() {}\n<redacted: ") and text.endswith("\n```"))
        self.assertEqual(stats["blocks"], 1)

    def test_signatures_across_languages(self):
        body = ["def run(argv):", "class Runner:", "lint *profile:", "[tool.lint]", "export function main() {",
                "build() {", "pub(crate) const MAX_STEPS: usize = 8;", "x = 1", "    return x", "let y = 2;"]
        self.assertEqual(jj.signature_lines(body), body[:7])

    def test_plan_gate_sends_a_fitting_bead_whole(self):
        text = "# Sprint\n\n" + RUST_BLOCK + "\n"
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text=text)
        sent = t.bodies[0]["state"]["item"]["text"]
        self.assertEqual(sent, text)  # a 14-line block is longer than --max-code-lines 8, but the bead fits
        self.assertNotIn("<redacted", sent)
        self.assertEqual(j.run["item"]["code_reduced"], {"blocks": 0, "lines": 0})
        self.assertEqual((j.run["item"]["chars"], j.run["item"]["chars_before_reduction"]), (len(text), len(text)))
        self.assertFalse(j.result["item_shortened"])
        self.assertNotIn("size", j.result)
        with tempfile.TemporaryDirectory() as d:  # the dry-run request carries the full code too
            j = Judged(d, Transport(), item_text=text, extra_args=["--dry-run"])
        self.assertEqual(j.run["requests"][0]["state"]["item"]["text"], text)
        self.assertEqual(j.run["item"]["code_reduced"], {"blocks": 0, "lines": 0})

    def test_oversize_plan_bead_reduces_only_its_longest_blocks(self):
        medium = "```rust\npub fn medium() {\n" + "    step();\n" * 200 + "}\n```"   # 202 lines, first in the bead
        large = "```rust\npub fn large() {\n" + "    step();\n" * 400 + "}\n```"     # 402 lines, the longest
        text = "# Sprint\n\n## Design\n\n" + medium + "\n\n" + RUST_BLOCK + "\n\n" + large + "\n"
        after_large = jj.reduce_code(text, 8, {2})[0]
        after_both = jj.reduce_code(text, 8, {0, 2})[0]
        self.assertGreater(len(after_large), 2000)
        self.assertLess(len(after_both), 2000)
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text=text, extra_args=["--max-item-chars", "2000"])
        sent = t.bodies[0]["state"]["item"]["text"]
        self.assertEqual(sent, after_both)
        self.assertIn("self.groups.iter()", sent)  # the 14-line block is long too, but was not needed
        self.assertEqual(sent.count("<redacted: "), 2)
        self.assertEqual(j.run["item"]["code_reduced"], {"blocks": 2, "lines": 201 + 401})
        self.assertEqual(j.run["item"]["chars_before_reduction"], len(text))
        self.assertEqual(j.run["item"]["chars"], len(after_both))
        with tempfile.TemporaryDirectory() as d:  # a looser budget stops after the longest block
            t = Transport()
            j = Judged(d, t, item_text=text, extra_args=["--max-item-chars", str(len(after_large))])
        self.assertEqual(t.bodies[0]["state"]["item"]["text"], after_large)
        self.assertEqual(j.run["item"]["code_reduced"], {"blocks": 1, "lines": 401})

    def test_request_bytes_limit_alone_triggers_reduction(self):
        large = "```rust\npub fn large() {\n" + "    step();\n" * 400 + "}\n```"
        text = "# Sprint\n\n" + large + "\n\n" + RUST_BLOCK + "\n"
        self.assertLess(len(text), jj.DEFAULT_MAX_ITEM_CHARS)
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(), item_text=text, extra_args=["--max-request-bytes", "4000", "--dry-run"])
        self.assertEqual(j.code, 0, j.out)
        request = j.run["requests"][0]
        self.assertLessEqual(len(jj.encode_body(request)), 4000)
        self.assertEqual(request["state"]["item"]["text"], jj.reduce_code(text, 8, {0})[0])
        self.assertIn("self.groups.iter()", request["state"]["item"]["text"])
        self.assertEqual(j.run["item"]["code_reduced"], {"blocks": 1, "lines": 401})
        with tempfile.TemporaryDirectory() as d:  # same bead, default request budget: sent whole
            j = Judged(d, Transport(), item_text=text, extra_args=["--dry-run"])
        self.assertEqual(j.run["requests"][0]["state"]["item"]["text"], text)

    def test_bead_still_too_large_after_every_long_block_is_reduced(self):
        large = "```rust\npub fn large() {\n" + "    step();\n" * 400 + "}\n```"
        text = "# T\n\n## Description\n\n" + "prose " * 600 + "\n\n" + large + "\n\n" + RUST_BLOCK + "\n"
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text=text, extra_args=["--max-item-chars", "3000"])
        self.assertEqual((j.code, j.out["error"]["code"]), (2, "PLAN.TOO_LARGE"))
        self.assertIn("the plan-gate item budget is --max-item-chars 3000", j.out["error"]["message"])
        self.assertEqual(t.bodies, [])
        shape = j.out["data"]["shape"]
        self.assertEqual(shape["code_reduced"], {"blocks": 2, "lines": 401 + 8})
        self.assertEqual(shape["chars_before_reduction"], len(text))
        self.assertEqual(shape["chars"], len(jj.reduce_code(text, 8)[0]))

    def test_reduce_code_default_selection_is_unchanged(self):
        text = "a\n" + RUST_BLOCK + "\nb\n```text\n" + "\n".join(f"l{n}" for n in range(12)) + "\n```\nc\n```toml\nx = 1\n```"
        full = jj.reduce_code(text, 8)
        self.assertEqual(full[1], {"blocks": 2, "lines": 8 + 12})
        self.assertEqual(jj.reduce_code(text, 8, None), full)
        self.assertEqual(jj.reduce_code(text, 8, {0, 1, 2}), full)
        self.assertEqual(jj.reduce_code(text, 8, set()), (text, {"blocks": 0, "lines": 0}))
        only_second = jj.reduce_code(text, 8, {1})
        self.assertIn("self.groups.iter()", only_second[0])
        self.assertEqual(only_second[1], {"blocks": 1, "lines": 12})
        self.assertEqual(jj.long_code_blocks(text, 8), [0, 1])
        self.assertEqual(jj.long_code_blocks(text, 12), [0])

    def test_dev_gate_does_not_reduce_code(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(gate="dev")
            j = Judged(d, t, item_text=RUST_BLOCK, gate="dev")
        self.assertEqual(t.bodies[0]["state"]["item"]["text"], RUST_BLOCK)
        self.assertNotIn("code_reduced", j.run["item"])
        self.assertNotIn("chars_before_reduction", j.run["item"])

    def test_oversize_plan_bead_is_refused_and_nothing_is_sent(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text="# T\n\n## Description\n\n" + "prose " * 5000)
        self.assertEqual((j.code, j.out["error"]["code"]), (2, "PLAN.TOO_LARGE"))
        self.assertTrue(j.out["error"]["message"].startswith("plan bead bd-42 is 30021 characters after long code was reduced to signatures (30021 prose, 0 code; by section: Description 30017, (title) 5); "))
        self.assertIn("the plan-gate item budget is --max-item-chars 24000", j.out["error"]["message"])
        action = j.out["error"]["suggested_action"]
        self.assertIn("judge its cause", action)
        self.assertLess(action.index("no anchor, requirement or user-approved issue"), action.index("Split only approved work"))
        self.assertTrue(action.endswith("See references/plan-size-and-shape.md"))
        self.assertNotIn("split the bead", action)
        self.assertEqual(t.bodies, [])
        # the error carries the same numbers `shape` would report
        data = j.out["data"]
        self.assertEqual(data["item_id"], "bd-42")
        self.assertEqual(data["size"], jj.size_breakdown("# T\n\n## Description\n\n" + "prose " * 5000))
        self.assertEqual((data["shape"]["sections"], data["shape"]["prose_chars"], data["shape"]["chars_before_reduction"]),
                         ({"title": 5, "description": 30017}, 30021, 30021))
        self.assertNotIn("paragraphs", data["shape"])
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text="prose " * 1000, extra_args=["--max-request-bytes", "4000"])
        self.assertEqual((j.code, j.out["error"]["code"]), (2, "PLAN.TOO_LARGE"))
        self.assertIn("over --max-request-bytes 4000", j.out["error"]["message"])
        self.assertEqual(t.bodies, [])
        with tempfile.TemporaryDirectory() as d:  # dry run is refused the same way: nothing would fit
            j = Judged(d, Transport(), item_text="prose " * 1000, extra_args=["--max-request-bytes", "4000", "--dry-run"])
        self.assertEqual((j.code, j.out["error"]["code"]), (2, "PLAN.TOO_LARGE"))

    def test_size_breakdown_separates_sections_prose_and_code(self):
        text = "# T\n\n## Description\n\nwhy\n\n## Design\n\n```rust\nfn a() {}\n```\n"
        size = jj.size_breakdown(text)
        self.assertEqual(set(size["sections"]), {"(title)", "Description", "Design"})
        self.assertEqual(size["code_chars"], len("```rust\nfn a() {}\n```\n"))
        self.assertEqual(size["prose_chars"] + size["code_chars"], len(text))
        self.assertEqual(sum(size["sections"].values()) + len("```rust\n") + len("```\n"), len(text) + 1)

    def test_long_code_does_not_make_a_plan_bead_oversize(self):
        code = "```rust\npub fn big() {\n" + "    step();\n" * 3000 + "}\n```"
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text="# Sprint\n\n" + code)
        self.assertEqual((j.code, j.result["verdict"]), (0, "satisfied"))
        self.assertLess(len(t.bodies[0]["state"]["item"]["text"]), 200)

    def test_plan_gate_context_overflow_is_too_large_not_halved(self):
        with tempfile.TemporaryDirectory() as d:
            t = Transport(http(422, "max_tokens_exceeded"))
            j = Judged(d, t, item_text="y" * 6000)
        self.assertEqual((j.code, j.result["verdict"], j.result["error"]["code"]), (1, "error", "PLAN.TOO_LARGE"))
        self.assertIn("Jev rejected it for context size", j.result["error"]["message"])
        self.assertEqual(j.result["error"]["data"]["shape"]["chars"], 6000)
        self.assertEqual(len(t.bodies), 1)
        self.assertFalse(j.result["item_shortened"])
        self.assertEqual(j.result["attempts"], 1)
        self.assertEqual(j.run["summary"]["errors"], ["ANC-001"])

    def test_max_code_lines_must_be_positive(self):
        with tempfile.TemporaryDirectory() as d:
            j = Judged(d, Transport(), extra_args=["--max-code-lines", "0"])
        self.assertEqual(j.out["error"]["code"], "VALIDATION.INPUT")
        with tempfile.TemporaryDirectory() as d:
            t = Transport()
            j = Judged(d, t, item_text="# S\n\n" + RUST_BLOCK, extra_args=["--max-code-lines", "20"])
        self.assertEqual(j.run["item"]["code_reduced"], {"blocks": 0, "lines": 0})
        self.assertIn("self.groups.iter()", t.bodies[0]["state"]["item"]["text"])


SHARED_PARAGRAPH = ("All work in this sprint follows the repository conventions: every public command prints JSON by default, "
                    "errors are typed and actionable, and no change lands without a passing CI run on the main branch.")


def bead_json(bid, title, description="", design="", acceptance="", notes="", kind=None):
    b = {"id": bid, "title": title, "description": description, "design": design, "acceptance_criteria": acceptance, "notes": notes}
    if kind:
        b["issue_type"] = kind
    return [b]


class ShapeTests(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.TemporaryDirectory()
        beads = Path(self.d.name) / "beads"
        self.b1 = write(beads, "bd-1.json", bead_json("bd-1", "Sprint 1 runtime crates", description="Copy the crates per ANC-001 and REQ-014.\n\n" + SHARED_PARAGRAPH,
                                                      design="Design:\n\n" + RUST_BLOCK + "\n\n- copy\n- delete atm_* modules\n- run tests",
                                                      acceptance="| Check | Result |\n| --- | --- |\n| builds | yes |\n| tests | yes |", kind="sprint"))
        self.b2 = write(beads, "bd-2.json", bead_json("bd-2", "CLI thinning", description=SHARED_PARAGRAPH + "\n\nFixes #42 and ADR-0007; see github.com/acme/repo/issues/77.",
                                                      acceptance="1. prints JSON\n2. exits 0"))
        self.b3 = write(beads, "bd-3.json", bead_json("bd-3", "Docs pass", notes="Short note without any reference to an issue or an anchor."))
        self.anchors = write(self.d.name, "anchors.json", record(anchor(), anchor("ANC-002", statement="The CLI layer only parses arguments and prints JSON.", quote="keep the CLI thin")))

    def tearDown(self):
        self.d.cleanup()

    def test_shape_text_metrics(self):
        text = jj.compose_item_text(json.loads(Path(self.b1).read_text()))["text"]
        s = jj.shape_text(text, 8)
        self.assertEqual(set(s["sections"]), {"title", "description", "design", "acceptance_criteria"})
        self.assertEqual((s["fenced_blocks"], s["code_lines"], s["bullets"], s["table_rows"]), (1, 14, 3, 3))
        self.assertEqual(s["code_reduced"], {"blocks": 1, "lines": 8})
        self.assertLess(s["chars_after_reduction"], s["chars"])
        self.assertEqual(s["references"], {"github": [], "req": ["REQ-014"], "adr": [], "anc": ["ANC-001"]})
        self.assertFalse(s["no_issue_reference"])
        self.assertIn(SHARED_PARAGRAPH, s["paragraphs"])
        size = jj.size_breakdown(text)
        self.assertEqual((s["code_chars"], s["prose_chars"]), (size["code_chars"], size["prose_chars"]))
        self.assertEqual(sum(s["sections"].values()), sum(size["sections"].values()))
        self.assertEqual(jj.find_references("Fixes #42 and ADR-0007; see github.com/acme/repo/issues/77 and #42 again, not C#4 or x/#9"),
                         {"github": ["#42", "#77"], "req": [], "adr": ["ADR-0007"], "anc": []})

    def test_shape_directory_json_and_duplicates(self):
        code, out = run_cli(["shape", "--beads", str(Path(self.d.name) / "beads"), "--anchors", self.anchors])
        self.assertEqual(code, 0, out)
        data = out["data"]
        self.assertEqual([b["id"] for b in data["beads"]], ["bd-1", "bd-2", "bd-3"])
        by_id = {b["id"]: b for b in data["beads"]}
        self.assertTrue(by_id["bd-1"]["is_sprint"] and not by_id["bd-2"]["is_sprint"])
        self.assertEqual(by_id["bd-2"]["references"], {"github": ["#42", "#77"], "req": [], "adr": ["ADR-0007"], "anc": []})
        self.assertEqual((by_id["bd-3"]["no_issue_reference"], by_id["bd-3"]["sections"]), (True, {"title": 13, "notes": 70}))
        self.assertNotIn("paragraphs", by_id["bd-1"])
        self.assertEqual(len(data["duplicated_paragraphs"]), 1)
        dup = data["duplicated_paragraphs"][0]
        self.assertEqual((dup["paragraph"], dup["chars"], dup["beads"]), (SHARED_PARAGRAPH.lower()[:80], len(SHARED_PARAGRAPH), ["bd-1", "bd-2"]))
        t = data["totals"]
        self.assertEqual((t["beads"], t["sprints"], t["fenced_blocks"], t["no_issue_reference"], t["duplicated_paragraphs"]), (3, 1, 1, 1, 1))
        self.assertEqual(t["chars"], sum(b["chars"] for b in data["beads"]))
        self.assertLess(t["chars_after_reduction"], t["chars"])
        self.assertEqual((by_id["bd-1"]["unknown_anchor_references"], data["anchors_not_referenced"]), ([], ["ANC-002"]))

    def test_sprint_detection_and_plan_gate_columns(self):
        self.assertTrue(jj.is_sprint_bead({"issue_type": "sprint"}, "x", "y"))
        self.assertTrue(jj.is_sprint_bead({"labels": ["stage:sprint", "area:cli"]}, "x", "y"))
        self.assertTrue(jj.is_sprint_bead({}, "proj-h-3", "h-3: CLI thinning"))
        self.assertTrue(jj.is_sprint_bead({}, "bd-77", "h-14: last one"))
        self.assertTrue(jj.is_sprint_bead({}, "proj-h-3", "anything"))
        self.assertTrue(jj.is_sprint_bead({}, "bd-1", "Sprint 1 runtime"))
        self.assertFalse(jj.is_sprint_bead({"labels": ["stage:plan"]}, "bd-1", "CLI thinning"))
        self.assertFalse(jj.is_sprint_bead({}, "bd-1", "h3 has no dash so it is a plain title"))
        self.assertFalse(jj.is_sprint_bead({}, "bd-1", "Follow-up 3: the space before the number breaks the prefix rule"))
        beads = [{"id": f"proj-h-{n}", "title": f"h-{n}: sprint {n}", "description": "d" * 100, "design": "x" * 300, "acceptance_criteria": "a" * 50} for n in range(1, 4)]
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "children.json", beads)
            code, out = run_cli(["shape", "--children-json", path])
            self.assertEqual((code, out["data"]["totals"]["sprints"]), (0, 3))
            row = out["data"]["beads"][0]
            self.assertTrue(row["is_sprint"])
            self.assertEqual((row["description_chars"], row["acceptance_chars"]), (row["sections"]["description"], row["sections"]["acceptance_criteria"]))
            self.assertEqual((row["description_chars"], row["acceptance_chars"]), (118, 76))  # heading line + blank + body + blank, each line counted with its newline
            self.assertAlmostEqual(row["design_ratio"], round(row["sections"]["design"] / row["chars"], 2))
            self.assertGreater(row["design_ratio"], 0.5)
            code, md = run_cli(["shape", "--children-json", path, "--format", "md"])
            self.assertIn("3 bead(s), 3 sprint(s)", md)
            self.assertIn(f"| proj-h-1 | h-1: sprint 1 | {row['chars']} | {row['description_chars']} | {row['acceptance_chars']} | {row['design_ratio']:.2f} | ", md)

    def test_shape_other_sources_markdown_and_errors(self):
        children = write(self.d.name, "children.json", [json.loads(Path(self.b2).read_text())[0], json.loads(Path(self.b3).read_text())[0]])
        code, out = run_cli(["shape", "--children-json", children, "--show-json", self.b1, "--max-code-lines", "20"])
        self.assertEqual((code, [b["id"] for b in out["data"]["beads"]]), (0, ["bd-2", "bd-3", "bd-1"]))
        self.assertEqual(out["data"]["beads"][2]["code_reduced"], {"blocks": 0, "lines": 0})
        self.assertNotIn("anchors_not_referenced", out["data"])
        code, md = run_cli(["shape", "--beads", str(Path(self.d.name) / "beads"), "--format", "md"])
        self.assertEqual(code, 0)
        self.assertIn("| Bead | Title | Chars | Desc chars | AC chars | Design ratio | After reduction | Code lines | Prose lines | Blocks | Bullets | Table rows | References |", md)
        self.assertIn("| bd-1 | Sprint 1 runtime crates | ", md)
        self.assertRegex(md, r"\| bd-3 \| Docs pass \| 82 \| 0 \| 0 \| 0\.00 \| 82 \| ")
        self.assertIn("| bd-3 | Docs pass | ", md)
        self.assertIn("REQ-014, ANC-001 |", md)
        self.assertIn("### Paragraphs repeated across beads", md)
        self.assertIn(f"| {SHARED_PARAGRAPH.lower()[:80]} | {len(SHARED_PARAGRAPH)} | bd-1, bd-2 |", md)
        self.assertIn("3 bead(s), 1 sprint(s)", md)
        code, out = run_cli(["shape", "--beads", str(Path(self.d.name) / "nowhere")])
        self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"))
        code, out = run_cli(["shape", "--show-json", write(self.d.name, "bad.json", [1, 2])])
        self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MALFORMED"))
        empty = Path(self.d.name) / "empty"
        empty.mkdir()
        code, out = run_cli(["shape", "--beads", str(empty)])
        self.assertEqual((code, out["error"]["code"]), (2, "INPUT.EMPTY"))
        code, out = run_cli(["shape", "--show-json", self.b1, "--max-code-lines", "0"])
        self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
        self.assertEqual(sorted(p.name for p in Path(self.d.name).iterdir()), ["anchors.json", "bad.json", "beads", "children.json", "empty"])  # writes nothing


class CheckFixturesTests(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.TemporaryDirectory()
        a = anchor(fixtures={"pass": ["Sprint 2 copies the legacy repo/crates/runtime into crates/runtime."],
                             "fail": ["Sprint 2 implements a new runtime crate from scratch.", "Sprint 2 defers the runtime."]})
        self.anchors = write(self.d.name, "anchors.json", record(a, anchor("ANC-002", status="proposed")))

    def tearDown(self):
        self.d.cleanup()

    def check(self, *extra):
        with env():
            return run_cli(["check-fixtures", "--anchors", self.anchors, "--gate", "plan", *extra])

    def test_expected_verdicts_by_fixture_key(self):
        fixture = write(self.d.name, "answers.json", {
            "ANC-001:pass:1": plan_answers(),
            "ANC-001:fail:1": plan_answers(comp="no", conf="yes"),
            "*": plan_answers(comp="no"),
        })
        code, out = self.check("--id", "ANC-001", "--answers-file", fixture)
        self.assertEqual(code, 0)
        checks = out["data"]["checks"]
        self.assertEqual([(c["anchor_id"], c["kind"], c["index"], c["verdict"], c["expected_ok"]) for c in checks],
                         [("ANC-001", "pass", 1, "satisfied", True), ("ANC-001", "fail", 1, "contradicted", True), ("ANC-001", "fail", 2, "not_addressed", True)])
        self.assertEqual((out["data"]["mismatches"], out["data"]["pass"], out["data"]["runs"], out["data"]["path"], out["data"]["run_path"]), ([], True, [], None, None))

    def test_mismatch_exits_one_and_runs_go_under_fixtures_dir(self):
        fixture = write(self.d.name, "answers.json", {"*": plan_answers()})  # everything satisfied: fail fixtures mismatch
        run_dir = str(Path(self.d.name) / "runs")
        code, out = self.check("--all", "--answers-file", fixture, "--run-dir", run_dir)
        self.assertEqual(code, 1)
        self.assertEqual(out["data"]["mismatches"], ["ANC-001:fail:1", "ANC-001:fail:2"])
        self.assertEqual(out["data"]["path"], str(Path(run_dir) / "fixtures"))
        self.assertEqual(out["data"]["run_path"], out["data"]["path"])
        self.assertEqual(len(out["data"]["runs"]), 3)
        self.assertTrue(all(Path(p).is_file() and Path(p).parent.name == "fixtures" for p in out["data"]["runs"]))
        self.assertTrue(all("fixture_ANC-001_" in Path(p).name for p in out["data"]["runs"]))
        self.assertEqual(out["data"]["checks"][0]["run_path"], out["data"]["runs"][0])

    def test_no_fixtures_and_dry_run(self):
        code, out = self.check("--id", "ANC-002", "--dry-run")
        self.assertEqual((code, out["error"]["code"]), (2, "FIXTURE.NONE"))
        code, out = self.check("--all", "--dry-run")
        self.assertEqual(code, 0)
        self.assertEqual(out["data"]["totals"]["requests"], 3)
        self.assertEqual(out["data"]["requests"][0]["state"]["item"]["id"], "fixture:ANC-001:pass:1")
        self.assertEqual(out["data"]["requests"][0]["state"]["item"]["title"], "fixture")
        self.assertIsNone(out["data"]["checks"][0]["expected_ok"])
        code, out = self.check("--dry-run")
        self.assertEqual(out["error"]["code"], "VALIDATION.INPUT")

    def test_live_check_uses_the_policy_and_opt_in(self):
        with env():
            code, out = self.check("--id", "ANC-001")
        self.assertEqual(out["error"]["code"], "AUTH.CREDENTIALS")
        t = Transport(http(429), {"model": "jev", "answers": plan_answers()}, {"model": "jev", "answers": plan_answers(comp="no")},
                      {"model": "jev", "answers": plan_answers(conf="yes")})
        with env(**LIVE_ENV), mock.patch.object(jj, "call_jev", t), mock.patch.object(jj, "_sleep", lambda s: None):
            code, out = run_cli(["check-fixtures", "--anchors", self.anchors, "--gate", "plan", "--id", "ANC-001"])
        self.assertEqual(code, 0)
        self.assertEqual([c["verdict"] for c in out["data"]["checks"]], ["satisfied", "not_addressed", "contradicted"])
        self.assertEqual(len(t.bodies), 4)


class RetireUpdateTests(unittest.TestCase):
    def test_retire(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor(), anchor("ANC-002")))
            code, out = run_cli(["retire", "--anchors", path, "--id", "ANC-001", "--replaced-by", "ANC-002", "--reason", "reworded"])
            self.assertEqual((code, out["data"]["status"], out["data"]["version"]), (0, "retired", 2))
            rec = json.loads(Path(path).read_text())
            a = rec["anchors"][0]
            self.assertEqual(a["status"], "retired")
            self.assertEqual(a["history"][-1]["event"], "retired")
            self.assertEqual((a["history"][-1]["replaced_by"], a["history"][-1]["reason"]), ("ANC-002", "reworded"))
            code, out = run_cli(["retire", "--anchors", path, "--id", "ANC-001"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            code, out = run_cli(["retire", "--anchors", path, "--id", "ANC-002", "--replaced-by", "ANC-404"])
            self.assertEqual(out["error"]["code"], "ANCHOR.UNKNOWN")
            # retired anchors are skipped by --all and render still works
            self.assertEqual(jj.validate_record(rec), [])

    def test_update_fields_and_fixtures(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor()))
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--scope", "runtime crates only",
                                 "--deterministic-check", "git log --follow crates/runtime", "--fixture-pass", "copies the legacy repo", "--fixture-fail", "rewrites"])
            self.assertEqual(code, 0)
            self.assertEqual(sorted(out["data"]["changed"]), ["deterministic_check", "fixtures", "scope"])
            a = json.loads(Path(path).read_text())["anchors"][0]
            self.assertEqual((a["version"], a["scope"], a["fixtures"]), (2, "runtime crates only", {"pass": ["copies the legacy repo"], "fail": ["rewrites"]}))
            self.assertEqual((a["history"][-1]["event"], sorted(a["history"][-1]["fields"])), ("updated", ["deterministic_check", "fixtures", "scope"]))
            self.assertEqual(a["statement"], STATEMENT)
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--clear-fixtures", "--fixture-pass", "only this", "--summary", "s"])
            a = json.loads(Path(path).read_text())["anchors"][0]
            self.assertEqual((a["fixtures"], a["version"]), ({"pass": ["only this"], "fail": []}, 3))
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--summary", "s"])  # unchanged value
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            self.assertIn("--statement", out["error"]["suggested_action"])

    def test_update_statement_is_allowed_and_recorded(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor()))
            new = "The runtime crates are copies of the legacy repo's runtime crates with the legacy-specific modules deleted."
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--statement", new])
            self.assertEqual(code, 0, out)
            self.assertEqual((out["data"]["changed"], out["data"]["version"], out["data"]["lint"]), (["statement"], 2, {"warnings": []}))
            a = json.loads(Path(path).read_text())["anchors"][0]
            self.assertEqual((a["statement"], a["quote"]), (new, QUOTE))
            self.assertEqual(a["history"][-1]["event"], "updated")
            self.assertEqual(a["history"][-1]["fields"], ["statement"])
            self.assertEqual(a["history"][-1]["previous_statement"], STATEMENT)
            # the same statement again is "nothing to update"
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--statement", new])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            # a hedged rewording is recorded with warnings, unless --strict
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--statement", "The crates are probably copied from the legacy repo.", "--strict"])
            self.assertEqual((code, out["error"]["code"]), (2, "ANCHOR.LINT"))
            self.assertIn("hedged wording", out["error"]["message"])
            self.assertEqual(json.loads(Path(path).read_text())["anchors"][0]["statement"], new)  # nothing written
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--statement", "The crates are probably copied from the legacy repo."])
            self.assertEqual((code, out["data"]["version"], out["data"]["lint"]["warnings"]), (0, 3, ["hedged wording; state the requirement"]))
            # the statement may never become the quote, and the quote is immutable
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--statement", QUOTE.upper() + "."])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            self.assertIn("statement is the verbatim quote", out["error"]["message"])
            code, out = run_cli(["update", "--anchors", path, "--id", "ANC-001", "--quote", "other words"])
            self.assertEqual((code, out["error"]["code"], out["error"]["message"]), (2, "VALIDATION.INPUT", "quote is verbatim provenance and immutable"))
            self.assertEqual(json.loads(Path(path).read_text())["anchors"][0]["quote"], QUOTE)

    def test_promote_and_set_beads(self):
        with tempfile.TemporaryDirectory() as d:
            path = write(d, "anchors.json", record(anchor()))
            code, out = run_cli(["set-beads", "--anchors", path, "--id", "ANC-001", "--anchor-bead", "bd-10", "--plan-bead", "bd-11"])
            self.assertEqual(out["data"]["beads"], {"anchor": "bd-10", "plan_questions": "bd-11", "dev_questions": None})
            code, out = run_cli(["promote", "--anchors", path, "--id", "ANC-001", "--adr", "ADR-0007"])
            a = json.loads(Path(path).read_text())["anchors"][0]
            self.assertEqual((a["status"], a["promoted_to"], a["version"]), ("promoted", "ADR-0007", 2))
            code, out = run_cli(["render", "--anchors", path])
            md = Path(out["data"]["path"]).read_text()
            self.assertIn(f"Anchor: {STATEMENT}\n\n> {QUOTE}\n\nSaid by the user, planning session, 2026-10-09.", md)
            self.assertIn("| ID | Status | Summary | Stated |", md)
            self.assertIn("promoted to ADR-0007", md)
            self.assertIn("Beads: anchor `bd-10`", md)


class ComposeItemTests(unittest.TestCase):
    SHOW = [{"id": "bd-42", "title": "Sprint 2 runtime", "description": "Copy the legacy crates.", "acceptance_criteria": "- builds\n- tests pass", "notes": ""}]

    def test_sections_in_order_and_empty_omitted(self):
        composed = jj.compose_item_text(self.SHOW)
        self.assertEqual(composed["sections"], ["Description", "Acceptance criteria"])
        self.assertEqual(composed["text"], "# Sprint 2 runtime\n\n## Description\n\nCopy the legacy crates.\n\n## Acceptance criteria\n\n- builds\n- tests pass\n")
        self.assertEqual((composed["item_id"], composed["chars"]), ("bd-42", len(composed["text"])))
        self.assertEqual(jj.compose_item_text(self.SHOW[0])["title"], "Sprint 2 runtime")  # bare object accepted
        with_design = dict(self.SHOW[0], design="Keep the trait object-safe.")
        self.assertEqual(jj.compose_item_text(with_design)["sections"], ["Description", "Design", "Acceptance criteria"])

    def test_sections_and_for_gate(self):
        full = dict(self.SHOW[0], design="Keep the trait object-safe.", notes="Ask the lead about naming.")
        self.assertEqual(jj.compose_item_text(full, sections=["description", "acceptance_criteria"])["sections"], ["Description", "Acceptance criteria"])
        self.assertEqual(jj.compose_item_text(full, sections=["notes"])["text"], "# Sprint 2 runtime\n\n## Notes\n\nAsk the lead about naming.\n")
        self.assertEqual(jj.compose_item_text(full, sections=[])["text"], "# Sprint 2 runtime\n")
        with self.assertRaises(jj.JudgeError) as cm:
            jj.compose_item_text(full, sections=["design", "closing"])
        self.assertEqual(cm.exception.code, "VALIDATION.INPUT")
        with tempfile.TemporaryDirectory() as d:
            show = write(d, "show.json", [full])
            children = write(d, "children.json", [{"id": "bd-43", "title": "t", "status": "closed", "close_reason": "r"}])
            code, out = run_cli(["compose-item", "--show-json", show, "--children-json", children, "--for", "plan"])
            self.assertEqual((code, out["data"]["sections"]), (0, ["Description", "Acceptance criteria"]))
            self.assertEqual(out["data"]["composition"], {"for": "plan", "sections_requested": ["description", "acceptance_criteria"], "children": False})
            self.assertNotIn("Closed beads", out["data"]["text"])
            code, out = run_cli(["compose-item", "--show-json", show, "--children-json", children, "--for", "dev"])
            self.assertEqual(out["data"]["sections"], ["Description", "Design", "Acceptance criteria", "Notes", "Closed beads"])
            self.assertEqual(out["data"]["composition"], {"for": "dev", "sections_requested": ["description", "design", "acceptance_criteria", "notes"], "children": True})
            code, out = run_cli(["compose-item", "--show-json", show, "--sections", "design, notes"])
            self.assertEqual((out["data"]["sections"], out["data"]["composition"]["sections_requested"]), (["Design", "Notes"], ["design", "notes"]))
            code, out = run_cli(["compose-item", "--show-json", show])
            self.assertEqual(out["data"]["composition"], {"for": None, "sections_requested": "all", "children": False})
            code, out = run_cli(["compose-item", "--show-json", show, "--sections", "design", "--for", "plan"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            code, out = run_cli(["compose-item", "--show-json", show, "--sections", "summary"])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            code, out = run_cli(["compose-item", "--show-json", show, "--sections", " , "])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))

    def test_closed_children_appended(self):
        children = [{"id": "bd-43", "title": "copy crates", "status": "closed", "close_reason": "done in 3f2a"},
                    {"id": "bd-44", "title": "open one", "status": "open"},
                    {"id": "bd-45", "title": "no reason", "status": "closed"}]
        composed = jj.compose_item_text(self.SHOW, children)
        self.assertEqual(composed["sections"][-1], "Closed beads")
        self.assertIn("## Closed beads\n\n- bd-43 copy crates: done in 3f2a\n- bd-45 no reason:\n", composed["text"])
        self.assertNotIn("bd-44", composed["text"])
        self.assertEqual(jj.compose_item_text(self.SHOW, [])["sections"], ["Description", "Acceptance criteria"])

    def test_cli_out_file_stdin_and_bad_shapes(self):
        with tempfile.TemporaryDirectory() as d:
            show = write(d, "show.json", self.SHOW)
            children = write(d, "children.json", [{"id": "bd-43", "title": "t", "status": "closed", "close_reason": "r"}])
            out_path = str(Path(d) / "item.txt")
            code, out = run_cli(["compose-item", "--show-json", show, "--children-json", children, "--out", out_path])
            self.assertEqual((code, out["data"]["path"], out["data"]["item_id"], out["data"]["title"]), (0, out_path, "bd-42", "Sprint 2 runtime"))
            self.assertEqual(out["data"]["sections"], ["Description", "Acceptance criteria", "Closed beads"])
            self.assertEqual(Path(out_path).read_text(), jj.compose_item_text(self.SHOW, json.loads(Path(children).read_text()))["text"])
            self.assertNotIn("text", out["data"])
            code, out = run_cli(["compose-item", "--show-json", show])
            self.assertIsNone(out["data"]["path"])
            self.assertTrue(out["data"]["text"].startswith("# Sprint 2 runtime"))
            with mock.patch.object(sys, "stdin", io.StringIO(json.dumps(self.SHOW))):
                code, out = run_cli(["compose-item", "--show-json", "-"])
            self.assertEqual(out["data"]["item_id"], "bd-42")
            two = write(d, "two.json", self.SHOW + self.SHOW)
            code, out = run_cli(["compose-item", "--show-json", two])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MALFORMED"))
            not_array = write(d, "obj.json", {"id": "bd-43"})
            code, out = run_cli(["compose-item", "--show-json", show, "--children-json", not_array])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MALFORMED"))  # children must be an array


def make_run(d, name, gate, item, created_at, verdicts, title="", with_quote=True):
    """A run file; with_quote=False imitates a run written before the quote field existed."""
    results = [{"anchor_id": aid, "source_type": "ANC" if aid.startswith("ANC") else aid[:3], "source_ref": None if aid.startswith("ANC") else aid,
                "statement": STATEMENT if aid.startswith("ANC") else REQ_STATEMENT,
                "stated_by": "the user", "stated_at": "2026-10-09", "context": "planning session", "anchor_version": 1,
                "question_set": {"gate": gate, "version": 1, "overridden": False}, "deterministic_check": "git log --follow crates/runtime",
                "verdict": v, "answers": {}, "min_confidence": 0.9, "explanation": jj.explain(v, 0.9, gate) if v else "dry run"}
               for aid, v in verdicts.items()]
    for r in results:
        if with_quote:
            r["quote"] = QUOTE if r["source_type"] == "ANC" else r["statement"]
        if r["verdict"] == "error":
            r["error"] = {"code": "JEV.OVERLOADED", "message": "Jev HTTP 529; response body withheld", "recoverable": True, "suggested_action": "Retry"}
    failing = [a for a, v in verdicts.items() if v in jj.FAILING]
    run = {"schema": "plan-anchoring/run/1", "run_id": f"{gate}-{created_at}-{item}", "gate": gate, "created_at": created_at,
           "anchors_path": "anchors.json", "plan": "phase-current",
           "item": {"id": item, "title": title, "source": "f", "chars": 10, "truncated": False},
           "model_requested": "jev-latest", "model_answered": "jev-1.13.0", "threshold": 0.6, "dry_run": False, "fixture": False,
           "results": results, "summary": {"counts": {}, "failing": failing, "errors": [a for a, v in verdicts.items() if v == "error"],
                                           "pass": not failing and "error" not in verdicts.values()}}
    return write(d, name, run)


class ReportTests(unittest.TestCase):
    def test_line_format_quotes_the_statement(self):
        with tempfile.TemporaryDirectory() as d:
            path = make_run(d, "r.json", "plan", "bd-42", "2026-10-09T18:30:00+00:00",
                            {"ANC-012": "not_addressed", "REQ-014": "contradicted", "ANC-013": "satisfied", "ANC-014": "not_applicable"}, title="Sprint 2 runtime")
            code, md = run_cli(["report", "--run", path, "--format", "md"])
            self.assertEqual(code, 0)
            self.assertIn("## Anchor gate: plan · bd-42 - Sprint 2 runtime", md)
            self.assertIn("Run `plan-2026-10-09T18:30:00+00:00-bd-42`, model `jev-1.13.0`, threshold 0.6.", md)
            self.assertIn(f'> **bd-42 · ANC-012 · not addressed** Anchor: {STATEMENT} You said (planning session, Oct 9): "{QUOTE}." This bead does not carry the directive forward.', md)
            # register text: the quote is the statement, so the says-clause is skipped
            self.assertIn(f'> **bd-42 · REQ-014 · contradicted** Anchor: {REQ_STATEMENT} This bead proposes something the anchor rules out.', md)
            self.assertNotIn("says:", md)
            self.assertNotIn("ANC-013", md)
            code, out = run_cli(["report", "--run", path])
            self.assertEqual((out["data"]["pass"], out["data"]["failing"]), (False, ["ANC-012", "REQ-014"]))
            self.assertEqual(out["data"]["markdown"], md.rstrip("\n") + "\n")  # print() adds one newline
            code, md_all = run_cli(["report", "--run", path, "--format", "md", "--all"])
            self.assertIn("> **bd-42 · ANC-013 · satisfied**", md_all)
            self.assertIn("> **bd-42 · ANC-014 · not applicable**", md_all)

    def test_register_names_dev_checks_and_errors(self):
        self.assertEqual(jj.quote_statement("ends with ?"), '"ends with ?"')
        self.assertEqual(jj.quote_statement(" plain "), '"plain."')
        self.assertEqual((jj.sentence(" plain "), jj.sentence("done!")), ("plain.", "done!"))
        with tempfile.TemporaryDirectory() as d:
            path = make_run(d, "r.json", "dev", "sprint-2", "2026-10-22T10:15:00+00:00", {"ANC-012": "asserted_only", "ADR-0007": "satisfied", "ANC-020": "error"})
            run = json.loads(Path(path).read_text())
            run["results"][1]["statement"] = "The CLI layer only parses arguments and prints JSON."
            run["results"][1]["quote"] = "The CLI stays thin"
            Path(path).write_text(json.dumps(run))
            code, md = run_cli(["report", "--run", path, "--format", "md", "--all"])
            self.assertIn(f'> **sprint-2 · ANC-012 · asserted only** Anchor: {STATEMENT} You said (planning session, Oct 9): "{QUOTE}." Compliance is asserted', md)
            self.assertIn(">   Pair with the deterministic check: git log --follow crates/runtime", md)
            # a register anchor whose quote differs from its statement shows both
            self.assertIn('Anchor: The CLI layer only parses arguments and prints JSON. ADR-0007 (ADR register) says: "The CLI stays thin." The directive was followed and evidenced.', md)
            self.assertIn("> **sprint-2 · ANC-020 · error**", md)
            self.assertIn(">   Error JEV.OVERLOADED: Jev HTTP 529; response body withheld", md)
            self.assertEqual(code, 0)

    def test_all_passing_message_and_bad_inputs(self):
        with tempfile.TemporaryDirectory() as d:
            path = make_run(d, "r.json", "plan", "bd-1", "2026-10-09T18:30:00+00:00", {"ANC-001": "satisfied"})
            code, md = run_cli(["report", "--run", path, "--format", "md"])
            self.assertIn("All judged anchors are satisfied or not applicable.", md)
            code, out = run_cli(["report", "--run", ""])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"))
            code, out = run_cli(["report", "--run", d])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"))
            other = write(d, "other.json", {"schema": "something-else"})
            code, out = run_cli(["report", "--run", other, "--format", "md"])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MALFORMED"))


class CoverageTests(unittest.TestCase):
    def test_uncovered_latest_wins_and_dry_runs_ignored(self):
        with tempfile.TemporaryDirectory() as d:
            runs = Path(d) / "runs"
            make_run(runs, "a1.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "satisfied", "ANC-002": "not_applicable", "ANC-003": "contradicted"})
            make_run(runs, "a2.json", "plan", "bd-1", "2026-10-09T11:00:00+00:00", {"ANC-001": "not_addressed", "ANC-002": "not_applicable", "ANC-003": "satisfied"})
            make_run(runs, "b.json", "plan", "bd-2", "2026-10-09T10:30:00+00:00", {"ANC-001": "satisfied", "ANC-002": "not_applicable", "ANC-003": "contradicted"})
            make_run(runs, "dev.json", "dev", "bd-2", "2026-10-09T12:00:00+00:00", {"ANC-002": "satisfied"})
            dry = json.loads(Path(make_run(runs, "dry.json", "plan", "bd-3", "2026-10-09T13:00:00+00:00", {"ANC-002": "satisfied"})).read_text())
            dry["dry_run"] = True
            write(runs, "dry.json", dry)
            write(runs, "junk.json", "{not json")
            write(runs, "other.json", {"schema": "x"})
            code, out = run_cli(["coverage", "--runs", str(runs), "--gate", "plan"])
            data = out["data"]
            self.assertEqual((code, data["items"], data["runs_considered"]), (0, ["bd-1", "bd-2"], 2))
            self.assertEqual(data["matrix"]["ANC-001"], {"bd-1": "not_addressed", "bd-2": "satisfied"})  # a2 (later) replaced a1
            self.assertEqual(data["uncovered_anchors"], ["ANC-002"])  # not_applicable everywhere is uncovered on purpose
            self.assertEqual(data["contradicted"], {"ANC-003": ["bd-2"]})
            code, md = run_cli(["coverage", "--runs", str(runs), "--gate", "plan", "--format", "md"])
            self.assertIn("| Anchor | bd-1 | bd-2 | Covered |", md)
            self.assertIn("| ANC-001 | not addressed | satisfied | yes |", md)
            self.assertIn("| ANC-002 | not applicable | not applicable | no |", md)
            self.assertIn("Uncovered anchors (no item satisfies them): ANC-002", md)
            code, out = run_cli(["coverage", "--runs", str(runs)])
            self.assertEqual(out["data"]["items"], ["dev:bd-2", "plan:bd-1", "plan:bd-2"])  # gates never overwrite each other
            self.assertEqual(out["data"]["matrix"]["ANC-002"]["dev:bd-2"], "satisfied")
            self.assertNotIn("ANC-002", out["data"]["uncovered_anchors"])
            code, out = run_cli(["coverage", "--runs", str(runs), "--gate", "dev", "--format", "json"])
            self.assertEqual(out["data"]["items"], ["bd-2"])
            code, out = run_cli(["coverage", "--runs", str(Path(d) / "nowhere")])
            self.assertEqual((code, out["error"]["code"]), (2, "RUNS.NONE"))


class DiffTests(unittest.TestCase):
    def test_regressions_added_removed(self):
        with tempfile.TemporaryDirectory() as d:
            prev = make_run(d, "p.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00",
                            {"ANC-001": "satisfied", "ANC-002": "not_addressed", "ANC-003": "satisfied", "ANC-004": "contradicted", "ANC-005": "satisfied"})
            nxt = make_run(d, "n.json", "plan", "bd-1", "2026-10-09T11:00:00+00:00",
                           {"ANC-001": "contradicted", "ANC-002": "satisfied", "ANC-003": "error", "ANC-004": "error", "ANC-006": "satisfied"})
            nxt_data = json.loads(Path(nxt).read_text())
            nxt_data["item"]["chars"] = 34
            Path(nxt).write_text(json.dumps(nxt_data))
            code, out = run_cli(["diff", "--prev", prev, "--next", nxt])
            data = out["data"]
            self.assertEqual(code, 0)
            changes = {c["anchor_id"]: c for c in data["changed"]}
            self.assertFalse(changes["ANC-001"]["improved"])
            self.assertTrue(changes["ANC-002"]["improved"])
            self.assertFalse(changes["ANC-003"]["improved"])  # satisfied -> error is a regression, not an improvement
            self.assertFalse(changes["ANC-004"]["improved"])  # losing a verdict is never an improvement
            self.assertEqual(data["regressions"], ["ANC-001", "ANC-003", "ANC-004"])
            self.assertEqual(data["pass_to_fail"], ["ANC-001", "ANC-003"])  # ANC-004 was already failing: a regression in severity, not a pass lost
            self.assertEqual(data["fail_to_pass"], ["ANC-002"])
            self.assertNotIn("expected", data)
            self.assertEqual(data["added"], [{"anchor_id": "ANC-006", "verdict": "satisfied"}])
            self.assertEqual(data["removed"], [{"anchor_id": "ANC-005", "verdict": "satisfied"}])
            self.assertEqual((data["pass_before"], data["pass_after"], data["item"]), (False, False, "bd-1"))
            self.assertEqual(data["item_size"], {"before_chars": 10, "after_chars": 34, "delta_chars": 24, "grew": True})

    def test_diff_prefers_pre_reduction_size_and_reports_cuts(self):
        with tempfile.TemporaryDirectory() as d:
            prev = make_run(d, "p.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "not_addressed"})
            nxt = make_run(d, "n.json", "plan", "bd-1", "2026-10-09T11:00:00+00:00", {"ANC-001": "satisfied"})
            p, n = json.loads(Path(prev).read_text()), json.loads(Path(nxt).read_text())
            p["item"].update({"chars": 800, "chars_before_reduction": 1200})
            n["item"].update({"chars": 900, "chars_before_reduction": 1100})
            Path(prev).write_text(json.dumps(p))
            Path(nxt).write_text(json.dumps(n))
            code, out = run_cli(["diff", "--prev", prev, "--next", nxt])
            self.assertEqual(code, 0, out)
            self.assertEqual(out["data"]["item_size"], {"before_chars": 1200, "after_chars": 1100,
                                                         "delta_chars": -100, "grew": False})

    def test_expected_verdicts_tell_corrections_from_regressions(self):
        with tempfile.TemporaryDirectory() as d:
            prev = make_run(d, "p.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "satisfied", "ANC-002": "satisfied", "ANC-003": "not_addressed", "ANC-004": "satisfied"})
            nxt = make_run(d, "n.json", "plan", "bd-1", "2026-10-09T11:00:00+00:00", {"ANC-001": "not_addressed", "ANC-002": "not_addressed", "ANC-003": "not_addressed", "ANC-004": "satisfied"})
            expected = write(d, "expected.json", {"ANC-001": "not_addressed", "ANC-002": "satisfied", "ANC-003": "satisfied", "ANC-004": "satisfied", "ANC-009": "not_applicable"})
            code, out = run_cli(["diff", "--prev", prev, "--next", nxt, "--expected", expected])
            self.assertEqual(code, 0, out)
            data = out["data"]
            self.assertEqual(data["pass_to_fail"], ["ANC-001", "ANC-002"])  # a human reading says ANC-001's flip was right
            e = data["expected"]
            self.assertEqual((e["corrections"], e["regressions"], e["still_off"], e["all_match"]), (["ANC-001"], ["ANC-002"], ["ANC-003", "ANC-009"], False))
            cells = {c["anchor_id"]: c for c in e["cells"]}
            self.assertEqual(cells["ANC-009"], {"anchor_id": "ANC-009", "expected": "not_applicable", "prev": None, "next": None, "prev_matches": False, "next_matches": False})
            self.assertEqual((cells["ANC-004"]["prev_matches"], cells["ANC-004"]["next_matches"]), (True, True))
            code, out = run_cli(["diff", "--prev", prev, "--next", nxt, "--expected", write(d, "bad.json", {"ANC-001": "maybe"})])
            self.assertEqual((code, out["error"]["code"]), (2, "VALIDATION.INPUT"))
            code, out = run_cli(["diff", "--prev", prev, "--next", nxt, "--expected", write(d, "list.json", ["x"])])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MALFORMED"))
            code, out = run_cli(["diff", "--prev", prev, "--next", nxt, "--expected", str(Path(d) / "none.json")])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"))

    def test_mismatched_runs_refused(self):
        with tempfile.TemporaryDirectory() as d:
            a = make_run(d, "a.json", "plan", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "satisfied"})
            b = make_run(d, "b.json", "dev", "bd-1", "2026-10-09T10:00:00+00:00", {"ANC-001": "satisfied"})
            c = make_run(d, "c.json", "plan", "bd-2", "2026-10-09T10:00:00+00:00", {"ANC-001": "satisfied"})
            code, out = run_cli(["diff", "--prev", a, "--next", b])
            self.assertEqual((code, out["error"]["code"]), (2, "RUNS.MISMATCH"))
            code, out = run_cli(["diff", "--prev", a, "--next", c])
            self.assertEqual((code, out["error"]["code"]), (2, "RUNS.MISMATCH"))
            code, out = run_cli(["diff", "--prev", a, "--next", str(Path(d) / "missing.json")])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"))


class ErrorHandlingTests(unittest.TestCase):
    def test_load_json_rejects_non_files(self):
        with tempfile.TemporaryDirectory() as d:
            for bad in ("", d, str(Path(d) / "missing.json")):
                with self.assertRaises(jj.JudgeError) as cm:
                    jj.load_json(bad, "thing")
                self.assertEqual(cm.exception.code, "INPUT.MISSING", bad)
            bad_json = write(d, "bad.json", "{")
            with self.assertRaises(jj.JudgeError) as cm:
                jj.load_json(bad_json, "thing")
            self.assertEqual(cm.exception.code, "INPUT.MALFORMED")

    def test_every_file_argument_yields_json_errors(self):
        with tempfile.TemporaryDirectory() as d:
            missing = str(Path(d) / "none.json")
            for argv in (["validate", "--anchors", missing], ["render", "--anchors", missing], ["set-beads", "--anchors", missing, "--id", "x"],
                         ["promote", "--anchors", missing, "--id", "x", "--adr", "A"], ["retire", "--anchors", missing, "--id", "x"],
                         ["update", "--anchors", missing, "--id", "x", "--scope", "s"], ["report", "--run", missing],
                         ["diff", "--prev", missing, "--next", missing], ["compose-item", "--show-json", missing],
                         ["judge", "--anchors", missing, "--gate", "plan", "--all", "--item-id", "x", "--dry-run"],
                         ["check-fixtures", "--anchors", missing, "--gate", "plan", "--all", "--dry-run"]):
                code, out = run_cli(argv)
                self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"), argv)
            not_record = write(d, "list.json", [1, 2])
            code, out = run_cli(["validate", "--anchors", not_record])
            self.assertEqual(out["error"]["code"], "INPUT.MALFORMED")

    def test_missing_item_file_is_json_not_traceback(self):
        with tempfile.TemporaryDirectory() as d:
            anchors = write(d, "anchors.json", record(anchor()))
            with env():
                code, out = run_cli(["judge", "--anchors", anchors, "--gate", "plan", "--all", "--item-id", "x", "--item-file", str(Path(d) / "nope.txt"), "--dry-run"])
            self.assertEqual((code, out["error"]["code"]), (2, "INPUT.MISSING"))

    def test_unexpected_exceptions_become_script_internal(self):
        with mock.patch.object(jj, "cmd_validate", side_effect=RuntimeError("boom")):
            code, out = run_cli(["validate", "--anchors", "x"])
        self.assertEqual((code, out["error"]["code"], out["error"]["message"]), (2, "SCRIPT.INTERNAL", "RuntimeError: boom"))
        with mock.patch.object(jj, "cmd_validate", side_effect=KeyboardInterrupt):
            code, out = run_cli(["validate", "--anchors", "x"])
        self.assertEqual((code, out["canceled"]), (130, True))

    def test_env_numbers_are_checked(self):
        with env(PLAN_ANCHOR_CONFIDENCE_THRESHOLD="high"):
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf), self.assertRaises(SystemExit) as cm:
                jj._env_number("PLAN_ANCHOR_CONFIDENCE_THRESHOLD", 0.6, float)
            self.assertEqual(cm.exception.code, 2)
            self.assertEqual(json.loads(buf.getvalue())["error"]["code"], "VALIDATION.INPUT")
        with env(PLAN_ANCHOR_CONFIDENCE_THRESHOLD="0.7"):
            self.assertEqual(jj._env_number("PLAN_ANCHOR_CONFIDENCE_THRESHOLD", 0.6, float), 0.7)
        with env():
            self.assertEqual(jj._env_number("PLAN_ANCHOR_CONFIDENCE_THRESHOLD", 0.6, float), 0.6)

    def test_exit_codes(self):
        self.assertEqual(jj.exit_code_for("judge", {"success": True, "data": {"dry_run": False, "summary": {"pass": True}}}), 0)
        self.assertEqual(jj.exit_code_for("judge", {"success": True, "data": {"dry_run": False, "summary": {"pass": False}}}), 1)
        self.assertEqual(jj.exit_code_for("judge", {"success": True, "data": {"dry_run": True, "summary": {"pass": False}}}), 0)
        self.assertEqual(jj.exit_code_for("check-fixtures", {"success": True, "data": {"mismatches": ["a"]}}), 1)
        self.assertEqual(jj.exit_code_for("check-fixtures", {"success": True, "data": {"mismatches": []}}), 0)
        self.assertEqual(jj.exit_code_for("validate", {"success": False, "data": {}}), 1)
        self.assertEqual(jj.exit_code_for("report", "markdown text"), 0)

    def test_documented_question_sets_load(self):
        for gate in ("plan", "dev"):
            qs = jj.load_question_set(gate)
            self.assertEqual(qs["gate"], gate)
            for q in qs["questions"].values():
                self.assertEqual(set(q["criteria"]), set(OPTIONS))
        self.assertEqual(set(jj.load_question_set("dev")["roles"]), {"relevance", "compliance", "conflict", "evidence"})
        with self.assertRaises(jj.JudgeError) as cm:
            jj.load_question_set("plan", str(Path(__file__)))
        self.assertEqual(cm.exception.code, "INPUT.MALFORMED")


if __name__ == "__main__":
    unittest.main()


class ComposeHeadingTests(unittest.TestCase):
    def test_internal_headings_are_demoted_so_sections_count_whole_fields(self):
        show = [{"id": "x-1", "title": "x-1: t", "description": "## Goal\n\nbody " * 1 + "\n\n```\n## not a heading\n```\n\n# Deep\n\nmore", "acceptance_criteria": "### Must\n\nok"}]
        text = jj.compose_item_text(show)["text"]
        self.assertIn("## Description", text)
        self.assertIn("### Goal", text)
        self.assertNotIn("\n## Deep", text)
        self.assertIn("### Deep", text)         # a level-1 heading inside a field lands at level 3, never 2
        self.assertIn("```\n## not a heading\n```", text)
        self.assertIn("#### Must", text)
        size = jj.size_breakdown(text)
        self.assertNotIn("Goal", size["sections"])
        self.assertIn("Description", size["sections"])

    def test_no_redaction_marker_when_nothing_dropped(self):
        body = "\n".join(f"fn f{i}() {{}}" for i in range(12))
        text = f"# t\n\n## Design\n\n```rust\n{body}\n```\n"
        reduced, stats = jj.reduce_code(text, 8)
        self.assertNotIn("<redacted", reduced)
        self.assertEqual(stats["lines"], 0)
