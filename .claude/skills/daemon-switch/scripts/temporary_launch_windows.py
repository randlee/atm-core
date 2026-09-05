"""Windows command-line codec retained for selector and fixture compatibility only.

The Windows daemon-switch backend is an interactive-user Scheduled Task. It
does not support temporary launch overlays and deliberately has no SCM adapter.
"""

from __future__ import annotations

from typing import Sequence

from temporary_launch import TemporaryLaunchError


def parse_windows_command_line(command: str) -> list[str]:
    """Parse the narrow CommandLineToArgvW-compatible subset used by fixtures."""
    arguments: list[str] = []
    index = 0
    length = len(command)
    while index < length:
        while index < length and command[index] in " \t":
            index += 1
        if index == length:
            break
        argument: list[str] = []
        quoted = False
        while index < length:
            if command[index] in " \t" and not quoted:
                break
            slashes = 0
            while index < length and command[index] == "\\":
                slashes += 1
                index += 1
            if index < length and command[index] == '"':
                argument.extend("\\" * (slashes // 2))
                if slashes % 2:
                    argument.append('"')
                else:
                    quoted = not quoted
                index += 1
                continue
            argument.extend("\\" * slashes)
            if index == length or (command[index] in " \t" and not quoted):
                break
            argument.append(command[index])
            index += 1
        if quoted:
            raise TemporaryLaunchError("Windows service command has unmatched quotation marks")
        arguments.append("".join(argument))
        while index < length and command[index] in " \t":
            index += 1
    return arguments


def quote_windows_command_line(arguments: Sequence[str]) -> str:
    """Render argv losslessly for the paired parser, including Windows quote rules."""
    return " ".join(quote_windows_argument(argument) for argument in arguments)


def quote_windows_argument(argument: str) -> str:
    """Quote one argv item using the CommandLineToArgvW backslash convention."""
    if not argument or any(character in " \t\"" for character in argument):
        rendered = ['"']
        slashes = 0
        for character in argument:
            if character == "\\":
                slashes += 1
            elif character == '"':
                rendered.append("\\" * (slashes * 2 + 1))
                rendered.append('"')
                slashes = 0
            else:
                rendered.append("\\" * slashes)
                rendered.append(character)
                slashes = 0
        rendered.append("\\" * (slashes * 2))
        rendered.append('"')
        return "".join(rendered)
    return argument
