from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_read_concurrency_gates import check
from check_read_concurrency_gates import extract_fn_body
from check_read_concurrency_gates import READ_HANDLERS
from check_read_concurrency_gates import write_op_variants


class ReadConcurrencyGateTests(unittest.TestCase):
    def test_extract_fn_body_stops_at_matching_brace(self) -> None:
        source = 'async fn receive_messages<T>() { /* } */ let value = "{"; call(value); }'
        self.assertEqual(extract_fn_body(source, "receive_messages"), '{ /* } */ let value = "{"; call(value); }')

    def test_handler_names_match_the_checked_in_contract(self) -> None:
        self.assertEqual(READ_HANDLERS, ("list_messages", "peek_messages", "receive_messages", "doctor"))

    def test_write_op_variants_extracts_tuple_and_struct_variants(self) -> None:
        source = """pub(crate) enum WriteOp {
    UpsertMessage(Message),
    ApplyReadDisplayState { mailbox: MailboxId },
}
"""
        self.assertEqual(
            write_op_variants(source),
            {"UpsertMessage", "ApplyReadDisplayState"},
        )

    def test_pre_cutover_baseline_is_inert(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            router = root / "crates/atm-http-runtime/src/storage_and_nudge_router.rs"
            ops = root / "crates/atm-storage-rusqlite/src/writer/ops.rs"
            router.parent.mkdir(parents=True)
            ops.parent.mkdir(parents=True)
            router.write_text("struct BlockingCoreBridge;", encoding="utf-8")
            ops.write_text("pub(crate) enum WriteOp {\n    ListMessages(Query),\n}\n", encoding="utf-8")
            self.assertEqual(check(root), [])

    def test_post_cutover_rejects_writer_read_and_bridge(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            router = root / "crates/atm-http-runtime/src/storage_and_nudge_router.rs"
            ops = root / "crates/atm-storage-rusqlite/src/writer/ops.rs"
            router.parent.mkdir(parents=True)
            ops.parent.mkdir(parents=True)
            router.write_text(
                """struct AsyncMailboxRuntime;
async fn list_messages() { BlockingCoreBridge::run(); }
async fn peek_messages() {}
async fn receive_messages() {}
async fn doctor() {}
""",
                encoding="utf-8",
            )
            ops.write_text("pub(crate) enum WriteOp {\n    ListMessages(Query),\n}\n", encoding="utf-8")
            findings = check(root)
            self.assertTrue(any("ListMessages" in finding for finding in findings))
            self.assertTrue(any("BlockingCoreBridge" in finding for finding in findings))
