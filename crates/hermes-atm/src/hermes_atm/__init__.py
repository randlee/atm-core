"""Hermes-facing ATM graft composition.

The package consumes only the public ``atm_graft`` Python API.  The Hermes
profile supplies its connected Telegram adapter and event-loop capabilities;
this package owns receiver activation, chat binding, visible host notice, and
internal Telegram-session injection.
"""

from .runtime import HermesAtmRuntime, HermesAtmRuntimeError

__all__ = ["HermesAtmRuntime", "HermesAtmRuntimeError"]
