"""Render one template file with the sc-compose Python wheel.

Scripts and tests render through `render()` instead of running the
`sc-compose render` CLI: no var files, no stdout parsing, and the template's
own directory is the confining root, so the result never depends on the
caller's working directory. The wheel and the CLI are the same version, in the
`sc-compose >=` range in the package manifest.
"""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Mapping

import sc_compose

RenderError = sc_compose.ScComposeError


def render(template: str | Path, variables: Mapping[str, Any]) -> str:
    """The rendered text, newline-terminated as `sc-compose render` prints it.

    Always strict (`--strict`); there is no lenient render. A referenced
    variable that is missing or undeclared raises, naming the variable and the
    template. A `.json` template's output must parse, as the CLI's checked render requires;
    the wheel does not check it (sc-compose#787; delete this check when it does). Raises
    `RenderError`; `error_text(exc)` is its message.
    """
    path = Path(template).resolve()
    request = sc_compose.ComposeRequest(
        str(path.parent), sc_compose.ComposeMode.file(str(path)), vars_input=dict(variables),
        policy=sc_compose.ComposePolicy(strict_undeclared_variables=True))
    text = sc_compose.compose_file(request).rendered_text
    if path.name.removesuffix(".j2").endswith(".json"):
        try:
            json.loads(text)
        except ValueError as exc:
            raise sc_compose.ScRenderError(
                f"ERR_RENDER_JSON_MALFORMED: rendered output failed the checked-render contract: {exc}") from exc
    return text if text.endswith("\n") else text + "\n"


def error_text(exc: BaseException) -> str:
    """The sc-compose diagnostic (`ERR_...: ... | location=...`) of a `RenderError`."""
    return str(exc.args[0]) if exc.args else str(exc)
