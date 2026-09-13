from __future__ import annotations

from pathlib import Path
import json
import sys
import subprocess
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from generate_report_index import ReportIndexError
from generate_report_index import build_index as _build_index
from generate_report_index import build_pages, stamp_nav, NAV_START, NAV_END
from generate_report_index import write_or_check
from scripts import report_runtime


def build_index(reports: Path) -> str:
    """Supply a minimal page manifest for legacy parser fixtures."""
    manifest = reports / "procedures/manifest.json"
    if not manifest.exists():
        manifest.parent.mkdir(parents=True, exist_ok=True)
        procedures = []
        for procedure in ("benchmark", "benchmark-run-2", "campaign", "send-message-benchmark", "read-query-benchmark", "smoke-unknown", "smoke-localhost", "smoke-local-ip", "fuzz-unknown"):
            html = reports / "procedures" / procedure / "00000000.html"
            html.parent.mkdir(parents=True, exist_ok=True)
            html.write_text(f"<html>{procedure}</html>\n", encoding="utf-8")
            procedures.append({"procedure": procedure, "revisions": [{"rev": "0" * 40, "date": "2026-01-01", "html": f"procedures/{procedure}/00000000.html"}]})
        manifest.write_text(json.dumps({"schema_version": 1, "procedures": procedures}), encoding="utf-8")
    return _build_index(reports)


def write_envelope(root: Path, name: str, report_type: str, generated_at: str, host: str) -> None:
    reports = root / "site/reports"
    reports.mkdir(parents=True, exist_ok=True)
    (reports / f"{name}.html").write_text(f"<html>{name}</html>\n", encoding="utf-8")
    (reports / name).mkdir(exist_ok=True)
    (reports / name / "evidence.json").write_text("{}\n", encoding="utf-8")
    (reports / f"{name}.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "report_type": report_type,
                "generated_at": generated_at,
                "host_label": host,
                "report_html": f"{name}.html",
            }
        ),
        encoding="utf-8",
    )


def write_smoke_envelope(root: Path, platform: str, host: str, run: str) -> str:
    reports = root / "site/reports"
    report_dir = reports / "smoke" / platform / host / run
    report_dir.mkdir(parents=True)
    (report_dir / "index.html").write_text("<html>smoke</html>\n", encoding="utf-8")
    report_html = report_dir.relative_to(reports).joinpath("index.html").as_posix()
    (report_dir / "smoke.envelope.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "report_type": "smoke",
                "generated_at": "2026-08-08T04:00:00Z",
                "host_label": host,
                "report_html": report_html,
                "procedure": "smoke-local-ip" if "local-ip" in run else "smoke-localhost",
                "status": "PASS",
            }
        ),
        encoding="utf-8",
    )
    return report_html


class GenerateReportIndexTests(unittest.TestCase):
    def test_index_and_runtime_share_git_unavailable_revision_resolution(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            write_envelope(root, "agreement", "benchmark", "2026-08-08T04:00:00Z", "host")
            payload_path = reports / "agreement.json"
            payload = json.loads(payload_path.read_text(encoding="utf-8"))
            payload.update({"procedure": "benchmark", "source_revision": "a" * 40})
            payload_path.write_text(json.dumps(payload), encoding="utf-8")
            revisions = [
                {"rev": "1" * 40, "date": "2026-08-01", "html": "procedures/benchmark/11111111.html"},
                {"rev": "2" * 40, "date": "2026-08-05", "html": "procedures/benchmark/22222222.html"},
            ]
            for revision in revisions:
                page = root / "site/reports" / revision["html"]
                page.parent.mkdir(parents=True, exist_ok=True)
                page.write_text("<html>procedure</html>\n", encoding="utf-8")
            (reports / "procedures/manifest.json").write_text(
                json.dumps({"schema_version": 1, "procedures": [{"procedure": "benchmark", "revisions": revisions}]}),
                encoding="utf-8",
            )
            with mock.patch.object(
                report_runtime.subprocess,
                "run",
                return_value=subprocess.CompletedProcess([], 1, "", "git unavailable"),
            ):
                runtime_page = report_runtime.resolve_procedure_page(
                    "benchmark", "a" * 40, root=root, generated_at="2026-08-08T04:00:00Z"
                )
                index = _build_index(reports)
            self.assertIsNotNone(runtime_page)
            assert runtime_page is not None
            self.assertEqual(runtime_page.html, "procedures/benchmark/22222222.html")
            self.assertIn('href="procedures/benchmark/22222222.html"', index)
            self.assertNotIn("inferred from run date", index)

    def test_source_revision_resolves_to_newest_ancestor_without_inference(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)
            history = root / "history.txt"
            history.write_text("a\n", encoding="utf-8")
            subprocess.run(["git", "add", "history.txt"], cwd=root, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "a"], cwd=root, check=True)
            older = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
            history.write_text("b\n", encoding="utf-8")
            subprocess.run(["git", "commit", "-q", "-am", "b"], cwd=root, check=True)
            newer = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
            history.write_text("c\n", encoding="utf-8")
            subprocess.run(["git", "commit", "-q", "-am", "c"], cwd=root, check=True)
            source_revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
            reports = root / "site/reports"
            write_envelope(root, "ancestor", "benchmark", "2026-08-08T04:00:00Z", "host")
            payload_path = reports / "ancestor.json"
            payload = json.loads(payload_path.read_text(encoding="utf-8"))
            payload.update({"procedure": "benchmark", "source_revision": source_revision})
            payload_path.write_text(json.dumps(payload), encoding="utf-8")
            manifest = {
                "schema_version": 1,
                "procedures": [{"procedure": "benchmark", "revisions": [
                    {"rev": older, "date": "2026-08-01", "html": f"procedures/benchmark/{older[:8]}.html"},
                    {"rev": newer, "date": "2026-08-05", "html": f"procedures/benchmark/{newer[:8]}.html"},
                ]}],
            }
            for revision in (older, newer):
                page = reports / "procedures/benchmark" / f"{revision[:8]}.html"
                page.parent.mkdir(parents=True, exist_ok=True)
                page.write_text("<html>procedure</html>\n", encoding="utf-8")
            (reports / "procedures/manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            index = _build_index(reports)
            self.assertIn(f'href="procedures/benchmark/{newer[:8]}.html"', index)
            self.assertNotIn("inferred from run date", index)

    def test_source_revision_absent_uses_inferred_date_path(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            write_smoke_envelope(root, "windows", "FastPC4", "20260808T032327Z-pid1-localhost")
            index = build_index(root / "site/reports")
            self.assertIn("inferred from run date", index)

    def test_links_report_to_procedure_by_source_revision(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            write_envelope(root, "source-revision", "benchmark", "2026-08-08T04:00:00Z", "host")
            payload_path = root / "site/reports/source-revision.json"
            payload = json.loads(payload_path.read_text(encoding="utf-8"))
            payload.update({"procedure": "benchmark", "source_revision": "0" * 40})
            payload_path.write_text(json.dumps(payload), encoding="utf-8")
            index = build_index(root / "site/reports")
            self.assertIn('href="procedures/benchmark/00000000.html"', index)
            self.assertNotIn("inferred from run date", index)

    def test_links_historical_smoke_result_by_run_date_and_marks_inferred(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            write_smoke_envelope(root, "windows", "FastPC4", "20260808T032327Z-pid1-localhost")
            index = build_index(root / "site/reports")
            self.assertIn("inferred from run date", index)

    def test_dated_report_resolves_to_revision_in_effect_not_head(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            write_smoke_envelope(root, "windows", "FastPC4", "20260808T032327Z-pid1-localhost")
            manifest = {
                "schema_version": 1,
                "procedures": [{
                    "procedure": "smoke-localhost",
                    "revisions": [
                        {"rev": "1" * 40, "date": "2026-08-01", "html": "procedures/smoke-localhost/11111111.html"},
                        {"rev": "2" * 40, "date": "2026-09-01", "html": "procedures/smoke-localhost/22222222.html"},
                    ],
                }],
            }
            for revision in ("11111111", "22222222"):
                page = reports / "procedures/smoke-localhost" / f"{revision}.html"
                page.parent.mkdir(parents=True, exist_ok=True)
                page.write_text("<html>procedure</html>\n", encoding="utf-8")
            (reports / "procedures/manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            index = _build_index(reports)
            self.assertIn('href="procedures/smoke-localhost/11111111.html"', index)
            self.assertNotIn('href="procedures/smoke-localhost/22222222.html"', index)

    def test_every_committed_report_resolves_to_a_manifest_page(self) -> None:
        root = Path(__file__).resolve().parents[2]
        self.assertEqual(write_or_check(root, check=True), 0)

    def test_rejects_report_whose_procedure_has_no_page(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir); reports = root / "site/reports"; reports.mkdir(parents=True)
            write_envelope(root, "bad", "benchmark", "2026-08-01T00:00:00Z", "host")
            (reports / "procedures").mkdir()
            (reports / "procedures/manifest.json").write_text('{"schema_version": 1, "procedures": []}\n')
            with self.assertRaises(ReportIndexError): write_or_check(root, check=False)

    def test_rejects_report_whose_manifest_page_file_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir); reports = root / "site/reports"; reports.mkdir(parents=True)
            write_envelope(root, "missing-page", "benchmark", "2026-08-01T00:00:00Z", "host")
            manifest = {
                "schema_version": 1,
                "procedures": [{"procedure": "missing-page", "revisions": [{
                    "rev": "0" * 40,
                    "date": "2026-01-01",
                    "html": "procedures/benchmark/missing.html",
                }]}],
            }
            (reports / "procedures").mkdir()
            (reports / "procedures/manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(ReportIndexError, "page does not exist"):
                write_or_check(root, check=False)

    def test_rejects_site_without_procedure_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir); write_envelope(root, "bad", "benchmark", "2026-08-01T00:00:00Z", "host")
            with self.assertRaises(ReportIndexError): write_or_check(root, check=False)

    def test_accepts_source_revision_and_procedure_envelope_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            write_envelope(root, "explicit", "benchmark", "2026-08-08T04:00:00Z", "host")
            payload_path = root / "site/reports/explicit.json"
            payload = json.loads(payload_path.read_text(encoding="utf-8"))
            payload.update({"procedure": "benchmark", "source_revision": "0" * 40})
            payload_path.write_text(json.dumps(payload), encoding="utf-8")
            index = build_index(root / "site/reports")
            self.assertIn('href="procedures/benchmark/00000000.html"', index)
    def test_empty_input_renders_an_empty_index_and_no_history(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "site/reports").mkdir(parents=True)
            pages = build_pages(root / "site/reports")
            self.assertEqual(list(pages), ["index.html"])
            self.assertEqual(pages["index.html"].count("No reports available."), 1)

    def test_index_row_shows_latest_run_and_history_lists_every_run_newest_first(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            older = write_smoke_envelope(root, "windows", "FastPC4", "20260808T032327Z-pid1-localhost")
            newer = write_smoke_envelope(root, "macos", "rand-m5", "20260809T050000Z-pid2-localhost")
            envelope = root / "site/reports" / Path(newer).parent / "smoke.envelope.json"
            payload = json.loads(envelope.read_text(encoding="utf-8"))
            payload.update({"generated_at": "2026-08-09T05:00:00Z", "status": "FAIL"})
            envelope.write_text(json.dumps(payload), encoding="utf-8")
            build_index(root / "site/reports")  # seeds the fixture manifest
            pages = build_pages(root / "site/reports")
            index = pages["index.html"]
            self.assertEqual(index.count('<td class="family"><strong>Smoke</strong></td>'), 1)
            self.assertIn(" · localhost · rand-m5</span>", index)
            self.assertIn(f'href="{newer}"', index)
            self.assertNotIn(f'href="{older}"', index)
            self.assertIn('<span class="result fail">FAIL</span>', index)
            self.assertIn('href="history/smoke.html">2 runs<', index)
            history = pages["history/smoke.html"]
            self.assertIn('<a href="../">Reports</a>', history)
            self.assertLess(history.index(newer), history.index(older))
            self.assertIn('href="../procedures/smoke-localhost/00000000.html"', history)

    def test_stamp_inserts_after_body_once_and_replaces_an_old_stamp(self) -> None:
        nav = f"{NAV_START}<nav>one</nav>{NAV_END}\n"
        page = "<html>\n<body class=\"x\">\n<h1>r</h1>\n</body>\n</html>\n"
        stamped = stamp_nav(page, nav)
        self.assertEqual(stamped, "<html>\n<body class=\"x\">\n" + nav + "<h1>r</h1>\n</body>\n</html>\n")
        self.assertEqual(stamp_nav(stamped, nav), stamped)
        replaced = stamp_nav(stamped, f"{NAV_START}<nav>two</nav>{NAV_END}\n")
        self.assertEqual(replaced.count("<nav>"), 1)
        self.assertIn("<nav>two</nav>", replaced)

    def test_every_page_of_a_run_carries_result_and_test_plan_link(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            run = write_smoke_envelope(root, "macos", "rand-m5", "20260809T050000Z-pid2-localhost")
            run_dir = root / "site/reports" / Path(run).parent
            (run_dir / "localhost.html").write_text("<html><body><p>detail</p></body></html>\n", encoding="utf-8")
            build_index(root / "site/reports")  # seeds the fixture manifest
            pages = build_pages(root / "site/reports")
            for relative in (run, run.replace("index.html", "localhost.html")):
                page = pages[relative]
                self.assertIn(NAV_START, page)
                self.assertIn('href="../../../../history/smoke.html">Smoke history</a>', page)
                self.assertIn("Result: <strong", page)
                self.assertIn('Test plan: <a href="../../../../procedures/smoke-localhost/00000000.html">smoke-localhost @ 00000000</a>', page)
            self.assertIn('<a href="../">ATM</a> › <span>Reports</span>', pages["index.html"])
            self.assertIn('<a href="../">Reports</a> › <span>Smoke history</span>', pages["history/smoke.html"])

    def test_check_rejects_a_report_page_without_the_current_navigation(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            run = write_smoke_envelope(root, "macos", "rand-m5", "20260809T050000Z-pid2-localhost")
            build_index(root / "site/reports")
            write_or_check(root, check=False)
            self.assertEqual(write_or_check(root, check=True), 0)
            page = root / "site/reports" / run
            page.write_text("<html><body>regenerated without nav</body></html>\n", encoding="utf-8")
            with self.assertRaisesRegex(ReportIndexError, "stale report page navigation"):
                write_or_check(root, check=True)
            write_or_check(root, check=False)
            self.assertEqual(write_or_check(root, check=True), 0)

    def test_check_rejects_a_history_page_whose_report_is_gone(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            write_smoke_envelope(root, "windows", "FastPC4", "20260808T032327Z-pid1-localhost")
            build_index(root / "site/reports")
            write_or_check(root, check=False)
            self.assertEqual(write_or_check(root, check=True), 0)
            orphan = root / "site/reports/history/smoke-retired.html"
            orphan.write_text("<html>retired</html>\n", encoding="utf-8")
            with self.assertRaisesRegex(ReportIndexError, "stale history pages"):
                write_or_check(root, check=True)
            write_or_check(root, check=False)
            self.assertFalse(orphan.exists())

    def test_aggregates_benchmark_and_orders_entries_newest_first(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            # Two run envelopes intentionally point to the same aggregate HTML.
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            (reports / "benchmark.html").write_text("<html>benchmark</html>\n", encoding="utf-8")
            (reports / "benchmark").mkdir()
            for name, host, timestamp in (
                ("benchmark", "mac-arm64", "2026-07-01T00:00:00Z"),
                ("benchmark-run-2", "linux-x64", "2026-07-03T00:00:00Z"),
            ):
                source = reports / f"{name}.json"
                source.write_text(
                    json.dumps(
                        {
                            "schema_version": 1,
                            "report_type": "benchmark",
                            "generated_at": timestamp,
                            "host_label": host,
                            "report_html": "benchmark.html",
                            "procedure": "benchmark",
                        }
                    ),
                    encoding="utf-8",
                )
            write_envelope(root, "campaign", "fuzz", "2026-07-02T00:00:00Z", "mac-arm64")
            index = build_index(reports)
            self.assertEqual(index.count('href="benchmark.html"'), 1)
            self.assertIn(">2 runs<", index)
            self.assertLess(index.index("benchmark.html"), index.index("campaign.html"))
            history = build_pages(reports)["history/benchmark.html"]
            self.assertLess(history.index("linux-x64"), history.index("mac-arm64"))
            self.assertEqual(history.count('href="../benchmark.html"'), 2)

    def test_rejects_unsafe_or_incomplete_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            (reports / "bad.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "benchmark",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "machine name",
                        "report_html": "bad.html",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ReportIndexError, "host_label"):
                build_index(reports)

    def test_rejects_missing_html_and_evidence_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            (reports / "missing.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "fuzz",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "mac-arm64",
                        "report_html": "missing.html",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ReportIndexError, "missing report HTML"):
                build_index(reports)

    def test_accepts_a_benchmark_index_inside_its_evidence_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            reports = Path(tempdir) / "site/reports"
            evidence = reports / "send-message-benchmark"
            evidence.mkdir(parents=True)
            (evidence / "index.html").write_text("<html>benchmark index</html>\n", encoding="utf-8")
            (reports / "send-message-benchmark.json").write_text(json.dumps({
                "schema_version": 1, "report_type": "benchmark",
                "generated_at": "2026-08-24T00:00:00Z", "host_label": "rand-m5",
                "report_html": "send-message-benchmark/index.html",
            }), encoding="utf-8")
            index = build_index(reports)
        self.assertIn('href="send-message-benchmark/index.html"', index)

    def test_canonical_benchmark_index_supersedes_but_does_not_delete_run_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            reports = Path(tempdir) / "site/reports"
            evidence = reports / "send-message-benchmark"
            evidence.mkdir(parents=True)
            (evidence / "index.html").write_text("<html>index</html>\n", encoding="utf-8")
            (reports / "send-message-benchmark.json").write_text(json.dumps({
                "schema_version": 1, "report_type": "benchmark",
                "generated_at": "2026-08-24T00:00:00Z", "host_label": "rand-m5",
                "report_html": "send-message-benchmark/index.html",
            }), encoding="utf-8")
            old = evidence / "old.envelope.json"
            old.write_text(json.dumps({
                "schema_version": 1, "report_type": "benchmark",
                "generated_at": "2026-08-01T00:00:00Z", "host_label": "rand-m5",
                "report_html": "send-message-benchmark.html",
            }), encoding="utf-8")
            index = build_index(reports)
            self.assertTrue(old.exists())
        self.assertIn("send-message-benchmark/index.html", index)
        self.assertNotIn('href="send-message-benchmark.html"', index)

    def test_discovers_nested_run_envelope_without_treating_evidence_as_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            evidence = reports / "campaign"
            evidence.mkdir(parents=True)
            (reports / "campaign.html").write_text("<html>campaign</html>\n", encoding="utf-8")
            (evidence / "run-001.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "fuzz",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "mac-arm64",
                        "report_html": "campaign.html",
                    }
                ),
                encoding="utf-8",
            )
            (evidence / "metrics.json").write_text(
                json.dumps({"schema_version": 2, "accepted": 10}), encoding="utf-8"
            )
            index = build_index(reports)
            self.assertIn('href="campaign.html"', index)
            self.assertNotIn("metrics", index)

    def test_ignores_nested_report_type_artifact_without_envelope_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            evidence = reports / "read-query-benchmark"
            evidence.mkdir(parents=True)
            (reports / "read-query-benchmark.html").write_text(
                "<html>benchmark</html>\n", encoding="utf-8"
            )
            (reports / "read-query-benchmark.json").write_text(
                json.dumps({
                    "schema_version": 1,
                    "report_type": "benchmark",
                    "generated_at": "2026-07-01T00:00:00Z",
                    "host_label": "mac-arm64",
                    "report_html": "read-query-benchmark.html",
                }),
                encoding="utf-8",
            )
            (evidence / "family.json").write_text(
                json.dumps({"schema_version": 1, "report_type": "read-query-benchmark", "families": []}),
                encoding="utf-8",
            )
            index = build_index(reports)
            self.assertIn('href="read-query-benchmark.html"', index)

    def test_discovers_every_nested_smoke_run_as_a_browsable_master_link(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            first = write_smoke_envelope(
                root,
                "windows",
                "FastPC4",
                "20260808T032327Z-pid22208-localhost",
            )
            second = write_smoke_envelope(
                root,
                "macos",
                "rand-m4",
                "20260808T040000Z-pid19288-local-ip",
            )

            index = build_index(root / "site/reports")

            self.assertEqual(index.count('<td class="family"><strong>Smoke</strong></td>'), 1)
            self.assertIn(f'href="{second}"', index)  # newest run of the family
            self.assertNotIn(f'href="{first}"', index)
            history = build_pages(root / "site/reports")["history/smoke.html"]
            self.assertIn(f'href="../{first}"', history)
            self.assertIn(f'href="../{second}"', history)
            self.assertIn("<td>local ip</td>", history)
            self.assertIn("<td>localhost</td>", history)

    def test_rejects_smoke_envelope_outside_its_run_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            report_dir = reports / "smoke" / "windows" / "FastPC4" / "run"
            report_dir.mkdir(parents=True)
            (report_dir / "index.html").write_text("<html>smoke</html>\n", encoding="utf-8")
            (reports / "wrong-place.envelope.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "smoke",
                        "generated_at": "2026-08-08T04:00:00Z",
                        "host_label": "FastPC4",
                        "report_html": "smoke/windows/FastPC4/run/index.html",
                        "status": "PASS",
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ReportIndexError, "stored beside"):
                build_index(reports)

    def test_discovers_legacy_smoke_result_without_a_new_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            report_dir = reports / "smoke" / "windows" / "FastPC4" / "20260808T032327Z-pid1-localhost"
            report_dir.mkdir(parents=True)
            (report_dir / "index.html").write_text("<html>smoke</html>\n", encoding="utf-8")
            (report_dir / "localhost.json").write_text(
                json.dumps(
                    {
                        "feature": "localhost",
                        "platform": "windows",
                        "host": "FastPC4",
                        "run_id": "20260808T032327Z",
                        "status": "FAIL",
                        "cases": [],
                    }
                ),
                encoding="utf-8",
            )

            index = build_index(reports)

            self.assertIn('href="smoke/windows/FastPC4/20260808T032327Z-pid1-localhost/index.html"', index)
            self.assertIn("FAIL", index)

    def test_check_detects_stale_index(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "site/reports").mkdir(parents=True)
            write_or_check(root, check=False)
            self.assertEqual(write_or_check(root, check=True), 0)
            index_path = root / "site/reports/index.html"
            index_path.write_text(index_path.read_text(encoding="utf-8") + "<!-- stale -->\n", encoding="utf-8")
            with self.assertRaisesRegex(ReportIndexError, "stale"):
                write_or_check(root, check=True)

    def test_rejects_report_symlink_that_escapes_public_root(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir, tempfile.TemporaryDirectory() as outside:
            root = Path(tempdir)
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            outside_path = Path(outside) / "private.html"
            outside_path.write_text("private\n", encoding="utf-8")
            (reports / "leak.html").symlink_to(outside_path)
            (reports / "leak").mkdir()
            (reports / "leak.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "benchmark",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "mac-arm64",
                        "report_html": "leak.html",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ReportIndexError, "escapes"):
                build_index(reports)


if __name__ == "__main__":
    unittest.main()
