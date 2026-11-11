from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from stack_top import JsonValue, StackTop, StackTopMismatch, select_confirmed_stack_top, select_stack_top


def test_confirmed_top_agrees_and_ignores_pending():
    layers: list[JsonValue] = [{"pr": 1, "branch": "one", "head": "aaa", "linked": True},
              {"pr": 2, "branch": "two", "head": "bbb", "linked": False}]
    expected = StackTop(1, "one", "aaa")
    assert select_confirmed_stack_top(layers, [1]) == expected
    assert select_stack_top({"metadata": {"layers": layers}}, "trunk", lambda _: "ccc") == (expected.branch, expected.head)


def test_ledger_ahead_of_hosted_is_typed_mismatch():
    layers: list[JsonValue] = [{"pr": 1, "branch": "one", "head": "aaa", "linked": True},
              {"pr": 2, "branch": "two", "head": "bbb", "linked": True}]
    assert select_confirmed_stack_top(layers, [1]) == StackTopMismatch(StackTop(2, "two", "bbb"), 1)


def test_hosted_ahead_of_ledger_is_typed_mismatch():
    layers: list[JsonValue] = [{"pr": 1, "branch": "one", "head": "aaa", "linked": True}]
    assert select_confirmed_stack_top(layers, [1, 2]) == StackTopMismatch(StackTop(1, "one", "aaa"), 2)


def test_empty_ledger_and_hosted_stack_selects_trunk_via_wrapper():
    assert select_confirmed_stack_top([], []) == StackTop(None, None, None)
    assert select_stack_top({"metadata": {"layers": []}}, "trunk", lambda _: "abc") == ("trunk", "abc")


def test_passed_first_layer_wins_over_trunk_but_held_layers_do_not():
    first = {"pr": 1, "branch": "first", "head": "aaa", "linked": False}
    held = {"pr": 2, "branch": "held", "head": "bbb", "linked": False,
            "held_on": {"pr": 1, "branch": "first"}}
    assert select_stack_top({"metadata": {"layers": [first, held]}}, "trunk", lambda _: "ccc") == ("first", "aaa")
    assert select_stack_top({"metadata": {"layers": [held]}}, "trunk", lambda _: "ccc") == ("trunk", "ccc")
