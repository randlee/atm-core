from pathlib import Path


def test_bead_queues_delegates_to_atm_bd_queues() -> None:
    script = Path(__file__).resolve().parents[1] / "bead-queues"

    assert script.read_text().splitlines() == ["#!/bin/sh", 'exec atm-bd queues "$@"']
