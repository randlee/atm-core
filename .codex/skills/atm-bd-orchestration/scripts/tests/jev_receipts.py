"""Valid Jev receipts for sanity-jev test replies, written by the real client (never by hand)."""
import importlib.util
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve()
CLIENT = HERE.parents[1] / "jev_client.py"
KEY = "test-only-key"
_spec = importlib.util.spec_from_file_location("jev_client_for_tests", CLIENT)
assert _spec is not None and _spec.loader is not None
client = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(client)


def receipted(envelope):
    """The success envelope with `data.jev` = the client's receipt for a `written` answer that agrees with its findings
    (no: one finding; yes: none), its request bound to the deliverable and findings."""
    if not envelope.get("success"):
        return envelope
    data = envelope["data"]
    choice = "no" if data["findings"] else "yes"
    request = {"model": client.MODEL, "state": {"deliverable": data["deliverable"], "findings": data["findings"],
                                                "commit": data["commit_checked"]},
               "questions": {"written": {"type": "choice", "instructions": "Delivered?",
                                         "criteria": {"yes": "delivered", "no": "not delivered"}}}}
    response = {"model": client.MODEL, "answers": {"written": {
        "type": "choice", "choice": choice, "confidence": 0.9,
        "probabilities": {"yes": 0.9, "no": 0.1} if choice == "yes" else {"yes": 0.1, "no": 0.9}}}}
    return {**envelope, "data": {**data, "jev": client.receipt(request, response, KEY)}}


def split(envelope, choices):
    """The success envelope with `data.jev` = the client's part list for a split request answered `choices` in order,
    every part with its own receipt, exactly as `jev_client.py --assignment` prints `data.parts`."""
    data = envelope["data"]
    requests = [client.written_request(f"deliverable {data['deliverable']}", f"part {i} evidence",
                                       part=(f"{i} of {len(choices)}", ["a.py", "b.py"]))
                for i in range(1, len(choices) + 1)]
    responses = [{"model": client.MODEL, "id": f"r{data['deliverable']}-{i}", "answers": {"written": {
        "type": "choice", "choice": choice, "confidence": 0.9,
        "probabilities": {"yes": 0.9, "no": 0.1} if choice == "yes" else {"yes": 0.1, "no": 0.9}}}}
        for i, choice in enumerate(choices, 1)]
    return {**envelope, "data": {**data, "jev": client.parts(requests, responses, KEY)["parts"]}}


def env():
    """The environment a merge verifies receipts in."""
    return {**os.environ, "TYPESAFE_API_KEY": KEY}
