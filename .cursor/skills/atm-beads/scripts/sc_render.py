# pyright: strict
"""Render one template file with the sc-compose Python wheel.

Scripts and tests render through `render()` instead of running the
`sc-compose render` CLI: no var files, no stdout parsing, and the template's
own directory is the confining root, so the result never depends on the
caller's working directory. The wheel and the CLI are the same version, in the
`sc-compose >=` range in the package manifest.
"""
from __future__ import annotations

from pathlib import Path
from typing import Any, Mapping

import sc_compose

RenderError = sc_compose.ScComposeError


def render(template: str | Path, variables: Mapping[str, Any]) -> str:
    """The rendered text, newline-terminated as `sc-compose render` prints it.

    Always strict (`--strict`); there is no lenient render. A referenced
    variable that is missing or undeclared raises, naming the variable and the
    template. A `.json` template's output must parse: the wheel applies the CLI's
    checked-render contract (sc-compose 1.8.0, sc-compose#787). Raises `RenderError`;
    `error_text(exc)` is its diagnostic.
    """
    path = Path(template).resolve()
    request = sc_compose.ComposeRequest(
        str(path.parent), sc_compose.ComposeMode.file(str(path)), vars_input=dict(variables),
        policy=sc_compose.ComposePolicy(strict_undeclared_variables=True))
    text = sc_compose.compose_file(request).rendered_text
    return text if text.endswith("\n") else text + "\n"


def error_text(exc: BaseException) -> str:
    """The sc-compose diagnostic (`ERR_...: ... | location=...`) of a `RenderError`.

    The wheel's error carries the message in `args[0]` and, since 1.8.0, the `ERR_...` code in
    `args[1]`; the code is prefixed when the message does not already start with it, so callers
    see the shape the CLI prints.
    """
    if not exc.args:
        return str(exc)
    message = str(exc.args[0])
    code = exc.args[1] if len(exc.args) > 1 else None
    if isinstance(code, str) and code.startswith("ERR_") and not message.startswith(code):
        return f"{code}: {message}"
    return message
