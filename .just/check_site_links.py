#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import argparse
import sys

LINK_ATTRIBUTES = {
    "a": "href",
    "iframe": "src",
    "img": "src",
    "link": "href",
    "script": "src",
    "source": "src",
}
IGNORED_SCHEMES = frozenset({"data", "http", "https", "javascript", "mailto", "tel"})


@dataclass(frozen=True)
class LinkReference:
    page: Path
    line: int
    value: str


@dataclass(frozen=True)
class BrokenLink:
    reference: LinkReference
    resolved: Path
    reason: str


class LinkParser(HTMLParser):
    def __init__(self, page: Path) -> None:
        super().__init__(convert_charrefs=True)
        self.page = page
        self.references: list[LinkReference] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attribute = LINK_ATTRIBUTES.get(tag.lower())
        if attribute is None:
            return
        for name, value in attrs:
            if name.lower() == attribute and value is not None:
                self.references.append(LinkReference(self.page, self.getpos()[0], value))
                return

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.handle_starttag(tag, attrs)


def discover_pages(site_root: Path) -> list[Path]:
    return sorted((*site_root.rglob("*.html"), *site_root.rglob("*.xhtml")))


def parse_page(page: Path) -> list[LinkReference]:
    parser = LinkParser(page)
    parser.feed(page.read_text(encoding="utf-8"))
    parser.close()
    return parser.references


def is_ignored(value: str) -> bool:
    stripped = value.strip()
    if not stripped or stripped.startswith("#"):
        return True
    return urlsplit(stripped).scheme.lower() in IGNORED_SCHEMES


def resolve_reference(reference: LinkReference, site_root: Path) -> BrokenLink | None:
    value = reference.value.strip()
    if is_ignored(value):
        return None

    site_root = site_root.resolve()
    path_text = unquote(urlsplit(value).path)
    if path_text.startswith("/"):
        return BrokenLink(reference, site_root / path_text.lstrip("/"), "root-absolute path")

    target = reference.page.resolve() if not path_text else (reference.page.parent / path_text).resolve()
    try:
        target.relative_to(site_root)
    except ValueError:
        return BrokenLink(reference, target, "escapes site root")

    if target.is_dir():
        index = target / "index.html"
        if not index.is_file():
            return BrokenLink(reference, index, "directory has no index.html")
        return None
    if not target.is_file():
        return BrokenLink(reference, target, "target does not exist")
    return None


def scan_site(site_root: Path) -> tuple[int, list[BrokenLink]]:
    pages = discover_pages(site_root)
    broken: list[BrokenLink] = []
    for page in pages:
        for reference in parse_page(page):
            finding = resolve_reference(reference, site_root)
            if finding is not None:
                broken.append(finding)
    return len(pages), broken


def display_path(path: Path, repo_root: Path) -> str:
    try:
        return path.relative_to(repo_root).as_posix()
    except ValueError:
        return path.as_posix()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Verify local links in the generated Pages site")
    parser.add_argument("--root", help="repository root (defaults to the discovered repository)")
    parser.add_argument("--site-root", default="site", help="site root, relative to --root by default")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    repo_root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parents[1]
    site_root = Path(args.site_root)
    if not site_root.is_absolute():
        site_root = repo_root / site_root
    site_root = site_root.resolve()

    pages_scanned, broken = scan_site(site_root)
    for finding in broken:
        reference = finding.reference
        print(
            f"{display_path(reference.page, repo_root)}:{reference.line}: "
            f"{reference.value} -> {display_path(finding.resolved, repo_root)} ({finding.reason})"
        )
    print(f"site links: {pages_scanned} pages scanned, {len(broken)} broken links")
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
