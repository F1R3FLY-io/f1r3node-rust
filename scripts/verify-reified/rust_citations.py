"""Rust line citations in the Reified RSpaces documents and formal files.

A citation names a Rust source and one or more line ranges:

    path.rs:N              one line
    path.rs:N-M            lines N to M
    path.rs:N-M, P-Q and R a list of ranges of one source

In Markdown, each range of a list may be its own code span, for example
`path.rs:N-M`, `P-Q`. In a formal file, a list may continue on the next
comment line.

The path is repository-relative, or a suffix of exactly one source path at a
path-component boundary. The prefix "rs/" stands for "rspace++/src/rspace/".
A short path that names more than one source needs an entry in ALIASES.
Build output under target/ is not a source.

A citation is valid when its path names exactly one Rust source, and each
range lies inside the file and starts and ends on a non-blank line. A line
that is inserted above a cited range moves the range onto the blank line
before the cited item, so the last rule finds the usual drift.

check-model-map.py applies these rules to the formal files of the model map,
and doc_lint.py applies them to the campaign documents (rule rust-cite).
Python 3 standard library only.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

PREFIXES = {"rs/": "rspace++/src/rspace/"}
ALIASES = {
    "accounting/mod.rs": "rholang/src/rust/interpreter/accounting/mod.rs",
    "util/mod.rs": "rholang/src/rust/interpreter/util/mod.rs",
    "scan.rs": "rholang/src/rust/interpreter/spaces/disciplines/scan.rs",
    # The join engine and the prefix source of the PathMap store (P5e) share the name join.rs.
    "join.rs": "rholang/src/rust/interpreter/spaces/join.rs",
    "pathmap/join.rs": "rholang/src/rust/interpreter/spaces/pathmap/join.rs",
}
SKIP_DIRECTORIES = ("target", ".git")
_RANGE = r"\d+(?:-\d+)?"
_SEPARATOR = r"`?\s*(?:,|\s+and)\s*`?(?=\s*\d)"
CITATION_RE = re.compile(
    r"(?P<path>(?:[A-Za-z0-9_+.-]+/)*[A-Za-z0-9_+-]+\.rs):"
    rf"(?P<ranges>\s*{_RANGE}(?:{_SEPARATOR}\s*{_RANGE})*)"
)
RANGE_RE = re.compile(r"(\d+)(?:-(\d+))?")


def rust_sources(root: Path) -> list[str]:
    """Return the root-relative paths of the Rust sources, without build output."""
    found: list[str] = []
    for directory, subdirectories, files in os.walk(root):
        subdirectories[:] = sorted(d for d in subdirectories if d not in SKIP_DIRECTORIES)
        found.extend(str((Path(directory) / name).relative_to(root)) for name in files if name.endswith(".rs"))
    return sorted(found)


def resolve(path: str, sources: list[str], aliases: dict[str, str]) -> list[str]:
    """Return the Rust sources that a cited path can name (exactly one on success)."""
    if path in aliases:
        return [aliases[path]] if aliases[path] in sources else []
    for prefix, expansion in PREFIXES.items():
        if path.startswith(prefix):
            path = expansion + path[len(prefix):]
            break
    return [source for source in sources if source == path or source.endswith("/" + path)]


class Citations:
    """Checks the Rust line citations of texts against the sources under one root."""

    def __init__(self, root: Path, aliases: dict[str, str] | None = None) -> None:
        self.root = root
        self.aliases = ALIASES if aliases is None else aliases
        self.sources = rust_sources(root)
        self.lines: dict[str, list[str]] = {}

    def source_lines(self, source: str) -> list[str]:
        if source not in self.lines:
            self.lines[source] = (self.root / source).read_text(encoding="utf-8").splitlines()
        return self.lines[source]

    def check_offsets(self, text: str) -> tuple[int, list[tuple[int, str]]]:
        """Return the number of cited ranges and the problems as (text offset, message) pairs."""
        count = 0
        problems: list[tuple[int, str]] = []
        for match in CITATION_RE.finditer(text):
            offset = match.start()
            cited = match.group("path")
            candidates = resolve(cited, self.sources, self.aliases)
            if len(candidates) != 1:
                if candidates:
                    problems.append((offset, f"{cited} matches {len(candidates)} Rust sources; "
                                             f"add it to ALIASES in rust_citations.py"))
                else:
                    problems.append((offset, f"{cited} matches no Rust source"))
                continue
            lines = self.source_lines(candidates[0])
            for first_text, last_text in RANGE_RE.findall(match.group("ranges")):
                count += 1
                first = int(first_text)
                last = int(last_text) if last_text else first
                span = f"{candidates[0]}:{first}" + (f"-{last}" if last_text else "")
                if not 1 <= first <= last <= len(lines):
                    problems.append((offset, f"{span} is outside the file ({len(lines)} lines)"))
                elif not lines[first - 1].strip() or not lines[last - 1].strip():
                    edge = "starts" if not lines[first - 1].strip() else "ends"
                    problems.append((offset, f"{span} {edge} on a blank line; cite the code lines"))
        return count, problems

    def check(self, text: str) -> tuple[int, list[tuple[int, str]]]:
        """Return the number of cited ranges and the problems as (line, message) pairs."""
        count, problems = self.check_offsets(text)
        return count, [(text.count("\n", 0, offset) + 1, message) for offset, message in problems]
