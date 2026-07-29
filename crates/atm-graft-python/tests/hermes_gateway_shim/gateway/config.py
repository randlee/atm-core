"""The small subset of Hermes gateway.config used by the adapter contract."""

from enum import Enum


class Platform(Enum):
    LOCAL = "local"
    TELEGRAM = "telegram"
