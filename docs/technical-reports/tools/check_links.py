#!/usr/bin/env python3
"""Relative link, image, and anchor checker for the technical-reports tree.

For every Markdown file passed on the command line (default: the two
technical reports and this tree's README), extract each inline link and image
target and assert:

  * a relative file target resolves to an existing file in the repository;
  * a `#fragment` on a relative target names a heading that exists in the
    target file (GitHub's anchor algorithm, duplicate-suffix rules included);
  * an in-document `#fragment` names a heading in the same file.

External (`http://`, `https://`, `mailto:`) targets are skipped — DOI
resolution belongs to `validate.sh`'s network check. Exit status is non-zero
if any reference fails. This complements `validate.sh`, whose link checks
(9–10) are suite-level and do not run on extra pages.

Usage:
  python3 docs/technical-reports/tools/check_links.py [FILE.md ...]
"""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent

_spec = importlib.util.spec_from_file_location('mk_tables', TOOLS_DIR / 'mk_tables.py')
if _spec is None or _spec.loader is None:
    raise SystemExit('cannot load mk_tables.py beside this script')
mk_tables = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(mk_tables)

REPO_ROOT = mk_tables.REPO_ROOT
TR_DIR = mk_tables.TR_DIR

DEFAULT_FILES = [
    TR_DIR / 'stack-safety/stack-safety-technical-report-2026-08-12.md',
    TR_DIR / 'pathmap/pathmap-technical-report-2026-08-12.md',
    TR_DIR / 'README.md',
]

# inline links and images: [text](target) / ![alt](target); targets with
# titles ("...") are split off. Reference-style links are not used in this
# tree and are not handled.
LINK_RE = re.compile(r'!?\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)')

# fenced-code regions must not contribute link candidates
FENCE_RE = re.compile(r'^\s*(```|~~~)')


def anchors_of(path: Path, cache: dict[Path, set[str]]) -> set[str]:
    if path not in cache:
        cache[path] = {h.slug for h in
                       mk_tables.parse_headings(mk_tables.read_lines(path))}
    return cache[path]


def candidates(path: Path) -> list[tuple[int, str]]:
    out = []
    in_fence = False
    for lineno, line in enumerate(mk_tables.read_lines(path), start=1):
        if FENCE_RE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        for m in LINK_RE.finditer(line):
            out.append((lineno, m.group(1)))
    return out


def check_file(path: Path, cache: dict[Path, set[str]]) -> list[str]:
    problems = []
    if not path.exists():
        return [f'{path}: file does not exist']
    for lineno, target in candidates(path):
        if target.startswith(('http://', 'https://', 'mailto:')):
            continue
        if target.startswith('#'):
            frag = target[1:]
            if frag not in anchors_of(path, cache):
                problems.append(f'{path.relative_to(REPO_ROOT)}:{lineno}: '
                                f'in-document anchor #{frag} not found')
            continue
        file_part, _, frag = target.partition('#')
        resolved = (path.parent / file_part).resolve()
        if not resolved.exists():
            problems.append(f'{path.relative_to(REPO_ROOT)}:{lineno}: '
                            f'target {target} -> {resolved} does not exist')
            continue
        if frag:
            if resolved.suffix.lower() != '.md':
                problems.append(f'{path.relative_to(REPO_ROOT)}:{lineno}: '
                                f'fragment on non-markdown target {target}')
                continue
            if frag not in anchors_of(resolved, cache):
                problems.append(f'{path.relative_to(REPO_ROOT)}:{lineno}: '
                                f'anchor #{frag} not found in '
                                f'{resolved.relative_to(REPO_ROOT)}')
    return problems


def main(argv: list[str]) -> int:
    files = [Path(a).resolve() for a in argv] if argv else DEFAULT_FILES
    cache: dict[Path, set[str]] = {}
    all_problems = []
    for f in files:
        probs = check_file(f, cache)
        n = len(candidates(f)) if f.exists() else 0
        status = 'PASS' if not probs else 'FAIL'
        print(f'{status} {f.name}: {n} link/image references checked, '
              f'{len(probs)} problems')
        all_problems.extend(probs)
    for p in all_problems:
        print('  ' + p)
    return 1 if all_problems else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
