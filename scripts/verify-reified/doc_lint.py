#!/usr/bin/env python3
"""Lint the Reified RSpaces campaign documents against the pgmcp documentation
guidelines.

The pgmcp documentation guidelines are mostly semantic. This script enforces
the parts that a program can decide. Each phase of the campaign binds its
"-documented" acceptance criterion to one run of this script, so every rule
below is deterministic, needs no network access, and writes nothing outside
its scratch directory.

Usage
-----
    doc_lint.py [options] [FILE ...]

Without FILE arguments, the script lints every ``*.md`` file under
``docs/rspace/reified-rspaces/`` and every ``docs/rholang/21-*.md`` file.
Files under an exempt directory (``design-record/``) are never linted: the
design record is history and is exempt from the STE and lint rules.

Each violation prints one line ``path:line: rule: detail``. With ``--json``,
the last line of standard output is a JSON summary. The exit status is 0
when there is no violation, 1 when there is at least one violation, and 2 on
a usage error. ``--list-rules`` prints the rule catalog.

Rules
-----
The rule names are stable. Tests and recipes assert them by name.

``markdown-fence``
    Every fenced code block is closed.
``math-delimiter``
    GitHub-flavored Markdown math uses ``$`...`$`` inline and a fence whose
    info string is ``math`` for display math. A ``$...$`` or ``$$...$$`` span,
    and any other bare dollar sign, is a violation, because GitHub strips
    backslash escapes from such spans before MathJax sees them. A literal
    dollar sign is written as inline code. A math fence needs no dollar
    delimiters, so a dollar sign inside a math fence is also a violation.
``math-abut``
    No ASCII letter abuts the opening delimiter of inline math.
``math-undelimited``
    A LaTeX control word (two or more letters after a backslash) outside a
    math span or code is mathematics that is not delimited as math.
``link``
    Every relative link, image, reference-style link, reference definition,
    and HTML ``href``/``src`` target resolves, and every fragment names an
    anchor of the target (a GitHub heading slug or an explicit ``id``).
``glossary-anchor``
    Every link to ``glossary.md#anchor`` resolves to an entry of that
    glossary. These links are reported here and not under ``link``.
``diagram-source``
    Every embedded ``diagrams/*.svg`` has a ``.puml`` source beside it.
``diagram-orphan``
    Every ``.puml`` source of a diagrams directory in scope is embedded (as
    its ``.svg``) or linked by at least one document.
``diagram-palette``
    Every ``.puml`` source includes the shared house palette and uses at
    least one of its color variables.
``diagram-render``
    ``render.sh --check`` passes for every diagrams directory in scope: each
    committed SVG equals a fresh render, and PlantUML reports no error and
    writes nothing to stdout or stderr.
``mermaid``
    A Mermaid diagram is allowed only for the diagram types in
    ``MERMAID_ALLOWLIST``, which PlantUML cannot express.
``ste-semicolon``, ``ste-contraction``
    Prose has no semicolon and no contraction.
``ste-sentence-length``
    A descriptive sentence has at most 25 words.
``ste-procedure-length``
    A procedural sentence has at most 20 words. A procedural sentence is a
    sentence of a list item that starts with an imperative verb, or with a
    condition, a comma, and an imperative verb.
``ste-paragraph-length``
    A paragraph or a list item has at most 6 sentences.
``snippet-corpus``
    Every ``rholang`` (or ``rho``) code block equals, after whitespace
    normalization, one row of a listed corpus file. A listed corpus file that
    does not exist is a violation for every document that has such a block.
``citation-doi``
    Every DOI has the form ``10.<registrant>/<suffix>``, and a link whose
    text shows a DOI targets the same DOI. DOI existence needs the network,
    so a host-side review step checks it.
``citation-key``
    Every in-text citation (a link to ``#ref-<key>``) has a reference entry
    (an element with ``id="ref-<key>"``) in the target document.
``rocq-cite``
    Every citation ``File.v:NNN`` in inline code is paired with the nearest
    preceding inline-code identifier on the same line or in the same
    sentence, as ``scripts/audit/check-rocq-line-refs.sh`` does. A
    declaration of that name must start at line NNN of the file. A bare
    ``File.v`` citation that follows ``name`` with only connective text
    between them (for example "in", "from", "(") requires only that the
    declaration exists. A line citation without a preceding identifier is a
    violation, because nothing can verify it.
``rust-cite``
    Every Rust line citation ``path.rs:N``, ``path.rs:N-M`` or a list such
    as ``path.rs:N-M``, ``P-Q`` outside fenced code names exactly one Rust
    source of the repository, and each range lies inside the file and starts
    and ends on a non-blank line. ``rust_citations.py`` defines the rules,
    the ``rs/`` prefix and the aliases of short paths. A line that is
    inserted above a cited range moves the range onto a blank line, so the
    rule finds the usual drift.
``placement``
    Inside the campaign document root, documents and directories have
    kebab-case names, and ``README.md`` links every document. A diagram
    source is ``NN[a]-kebab-case.puml``: ``NN`` is the two-digit figure
    number, and ``[a]`` is an optional lowercase sub-figure letter (for
    example ``06a-…`` beside ``06-…``). Besides sources, includes and SVGs,
    ``diagrams/`` holds only the renderer files ``render.sh`` and
    ``fonts.lock``.

STE scope
---------
The STE rules apply to prose only. They ignore code blocks, inline code,
math, tables, headings, link targets, images, HTML tags, block quotes,
quotations in straight or curly double quotes, and the sections whose
heading is "References" or "Bibliography".
"""

from __future__ import annotations

import argparse
import bisect
import json
import os
import re
import subprocess
import sys
import unicodedata
import urllib.parse
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rust_citations  # noqa: E402

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 and earlier
    tomllib = None

SCRIPT_PATH = Path(__file__).resolve()
DEFAULT_REPO_ROOT = SCRIPT_PATH.parents[2]
DEFAULT_DOC_ROOT = Path("docs/rspace/reified-rspaces")
DEFAULT_CHAPTER_GLOB = ("docs/rholang", "21-*.md")
DEFAULT_CORPORA = (
    Path("test-vectors/reified-syntax-corpus-v1.json"),
    Path("examples/reified_rspaces/MANIFEST.toml"),
)
DEFAULT_THEORIES = Path("formal/rocq/reified_rspaces/theories")
DEFAULT_PALETTE = Path("docs/casper/theory/diagrams/palette.iuml")
DEFAULT_RENDER_SCRIPT = Path("docs/rspace/reified-rspaces/diagrams/render.sh")
EXEMPT_DIR_NAMES = frozenset({"design-record"})

RULES: dict[str, str] = {
    "markdown-fence": "Every fenced code block is closed.",
    "math-delimiter": "Inline math is $`...`$ and display math is a ```math fence; "
    "no $...$, no $$...$$, and no bare dollar sign (write a literal dollar sign as inline code).",
    "math-abut": "No ASCII letter abuts the opening delimiter $` of inline math.",
    "math-undelimited": "No LaTeX control word outside a math span or code.",
    "link": "Every relative link, image, reference link, HTML href/src and fragment resolves.",
    "glossary-anchor": "Every glossary.md#anchor link resolves to a glossary entry.",
    "diagram-source": "Every embedded diagrams/*.svg has a .puml source beside it.",
    "diagram-orphan": "Every .puml source is embedded or linked by a document.",
    "diagram-palette": "Every .puml source includes the shared palette and uses its colors.",
    "diagram-render": "render.sh --check passes (current SVG, no PlantUML error, no stdout or stderr output).",
    "mermaid": "No Mermaid diagram of a type that PlantUML can express.",
    "ste-semicolon": "No semicolon in prose.",
    "ste-contraction": "No contraction in prose.",
    "ste-sentence-length": "At most 25 words in a descriptive sentence.",
    "ste-procedure-length": "At most 20 words in a procedural sentence (list item that starts with an imperative verb).",
    "ste-paragraph-length": "At most 6 sentences in a paragraph or list item.",
    "snippet-corpus": "Every rholang code block is a row of a listed corpus file after whitespace normalization.",
    "citation-doi": "Every DOI is well formed (10.<registrant>/<suffix>) and a DOI link shows its target DOI.",
    "citation-key": "Every in-text citation #ref-<key> has a reference entry id=\"ref-<key>\".",
    "rocq-cite": "Every Rocq citation `File.v:NNN` names a declaration that starts at line NNN.",
    "rust-cite": "Every Rust citation `path.rs:N[-M]` names one source, inside the file, and starts and ends on code.",
    "placement": "Kebab-case document names, NN[a]-kebab-case.puml diagram sources, and a README.md index that links every document.",
}

# Mermaid diagram types that PlantUML cannot express. Each entry records the
# evidence: PlantUML 1.2026.8 rejects the start tag, and its chart package
# (net/sourceforge/plantuml/chart) provides only area, bar, line and scatter
# renderers on Cartesian axes.
MERMAID_ALLOWLIST: dict[str, str] = {
    "sankey-beta": "A Sankey diagram encodes flow magnitude as band width. PlantUML has no Sankey renderer (@startsankey is rejected).",
    "sankey": "The stable name of sankey-beta.",
    "pie": "A pie chart encodes shares as sector angles. PlantUML has no sector renderer (@startpie is rejected).",
    "radar-beta": "A radar chart needs polar axes. PlantUML charts are Cartesian (@startradar is rejected).",
    "treemap-beta": "A treemap encodes size as area. PlantUML has no area-proportional layout (@starttreemap is rejected).",
}

# PlantUML equivalents, used in the violation message for other Mermaid types.
MERMAID_EQUIVALENTS: dict[str, str] = {
    "flowchart": "an activity or component diagram",
    "graph": "an activity or component diagram",
    "sequencediagram": "a sequence diagram",
    "classdiagram": "a class diagram",
    "statediagram": "a state diagram",
    "statediagram-v2": "a state diagram",
    "erdiagram": "an IE or Chen entity-relationship diagram (@startchen)",
    "gantt": "a Gantt chart (@startgantt)",
    "mindmap": "a mind map (@startmindmap)",
    "gitgraph": "a git graph (@startgit)",
    "packet-beta": "a packet diagram (@startpacketdiag)",
    "packet": "a packet diagram (@startpacketdiag)",
    "xychart-beta": "a chart (@startchart: bar, line, area, scatter)",
    "quadrantchart": "a scatter chart with annotations (@startchart)",
    "timeline": "a chronology or Gantt chart",
    "journey": "an activity diagram or a chart (@startchart)",
    "c4context": "a C4-PlantUML diagram",
    "c4container": "a C4-PlantUML diagram",
    "c4component": "a C4-PlantUML diagram",
    "c4dynamic": "a C4-PlantUML diagram",
    "c4deployment": "a C4-PlantUML diagram",
    "block-beta": "a component or deployment diagram",
    "architecture-beta": "a deployment diagram",
    "kanban": "rectangles grouped in packages",
    "requirementdiagram": "a class diagram with stereotypes",
    "zenuml": "a sequence diagram",
}

# ---------------------------------------------------------------------------
# Violations
# ---------------------------------------------------------------------------


@dataclass(frozen=True, order=True)
class Violation:
    path: str
    line: int
    rule: str
    detail: str

    def render(self) -> str:
        return f"{self.path}:{self.line}: {self.rule}: {self.detail}"

    def as_json(self) -> dict:
        return {"path": self.path, "line": self.line, "rule": self.rule, "detail": self.detail}


# ---------------------------------------------------------------------------
# Markdown block model
#
# The parser is line-oriented and follows the CommonMark block structure
# closely enough for linting: fenced and indented code, ATX and setext
# headings, block quotes, list items, GFM tables, HTML blocks, reference
# definitions, thematic breaks, and paragraphs. HTML comments are blanked
# before block parsing, so their text is never linted. Every block keeps the
# 1-based number of its first line, and its content lines are consecutive
# source lines, so an offset in the joined block text maps back to a line.
# ---------------------------------------------------------------------------

FENCE_OPEN_RE = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,}|~{3,})(?P<info>.*)$")
HEADING_RE = re.compile(r"^ {0,3}(?P<hashes>#{1,6})(?:[ \t]+(?P<text>.*?))?[ \t]*$")
SETEXT_RE = re.compile(r"^ {0,3}(?P<ch>=+|-{2,})[ \t]*$")
THEMATIC_RE = re.compile(r"^ {0,3}(?:(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,})$")
LIST_RE = re.compile(r"^(?P<indent>[ \t]*)(?P<marker>[-*+]|\d{1,9}[.)])(?:[ \t]+(?P<rest>.*))?$")
QUOTE_RE = re.compile(r"^ {0,3}>[ ]?(?P<rest>.*)$")
REFDEF_RE = re.compile(r"^ {0,3}\[(?P<label>[^\]^][^\]]*)\]:[ \t]*(?P<dest><[^>]*>|\S+)(?P<title>.*)$")
TABLE_DELIM_RE = re.compile(r"^[ \t]*\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$")
HTML_ONLY_TAGS_RE = re.compile(r"^ {0,3}(?:<[^>]*>[ \t]*)+$")
# CommonMark HTML block type 7: a line that holds exactly one open or closing tag.
HTML_TYPE7_RE = re.compile(r"^ {0,3}(?:<[A-Za-z][A-Za-z0-9-]*(?:\s[^<>]*)?/?>|</[A-Za-z][A-Za-z0-9-]*\s*>)[ \t]*$")
HTML_BLOCK_START_RE = re.compile(
    r"^ {0,3}</?(?:address|article|aside|blockquote|center|details|dialog|div|dl|figcaption|"
    r"figure|footer|h[1-6]|header|hr|main|nav|ol|p|picture|pre|section|summary|table|tbody|td|"
    r"tfoot|th|thead|tr|ul|img|source|br|video|audio)\b",
    re.IGNORECASE,
)
REFERENCE_SECTION_TITLES = frozenset({"references", "bibliography", "works cited"})


@dataclass
class Block:
    kind: str  # paragraph, list, heading, table, quote, html, refdef, code, rule
    start: int  # 1-based line number of the first content line
    lines: list[str]
    info: str = ""  # code: the fence info string
    level: int = 0  # heading level
    closed: bool = True  # code: the closing fence exists
    ste_exempt: bool = False  # inside a References or Bibliography section

    @property
    def text(self) -> str:
        return "\n".join(self.lines)


def blank_html_comments(lines: list[str]) -> list[str]:
    """Replace HTML comment text with spaces. Code fences are respected."""
    out: list[str] = []
    in_comment = False
    fence: tuple[str, int] | None = None
    for line in lines:
        if fence is None and not in_comment:
            m = FENCE_OPEN_RE.match(line)
            if m and not ("`" in m.group("info") and m.group("fence")[0] == "`"):
                fence = (m.group("fence")[0], len(m.group("fence")))
                out.append(line)
                continue
        elif fence is not None:
            stripped = line.strip()
            if stripped and set(stripped) == {fence[0]} and len(stripped) >= fence[1]:
                fence = None
            out.append(line)
            continue
        chars = list(line)
        i = 0
        while i < len(chars):
            if in_comment:
                if line.startswith("-->", i):
                    for k in range(i, i + 3):
                        chars[k] = " "
                    in_comment = False
                    i += 3
                    continue
                chars[i] = " "
                i += 1
                continue
            if line.startswith("<!--", i):
                in_comment = True
                for k in range(i, i + 4):
                    chars[k] = " "
                i += 4
                continue
            i += 1
        out.append("".join(chars))
    return out


def parse_markdown(text: str) -> list[Block]:
    lines = blank_html_comments([ln.rstrip("\r") for ln in text.split("\n")])
    blocks: list[Block] = []
    cur: Block | None = None
    in_list = False
    in_table = False
    prev_blank = True
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        ln = i + 1
        stripped = line.strip()

        # An HTML block of type 6 or 7 runs until a blank line, so a heading or
        # a fence right after it is raw HTML text, not Markdown.
        if cur is not None and cur.kind == "html" and cur.info == "continues" and stripped:
            cur.lines.append(line)
            i += 1
            continue

        m = FENCE_OPEN_RE.match(line)
        if m and not (m.group("fence")[0] == "`" and "`" in m.group("info")):
            ch, size = m.group("fence")[0], len(m.group("fence"))
            body: list[str] = []
            j = i + 1
            closed = False
            while j < n:
                s = lines[j].strip()
                if s and set(s) == {ch} and len(s) >= size:
                    closed = True
                    break
                body.append(lines[j])
                j += 1
            blocks.append(Block("code", ln + 1, body, info=m.group("info").strip(), closed=closed))
            if not closed:
                blocks[-1].start = ln
            cur = None
            in_table = False
            prev_blank = False
            i = j + 1
            continue

        if not stripped:
            cur = None
            in_table = False
            prev_blank = True
            i += 1
            continue

        indent = len(line) - len(line.lstrip(" \t"))
        if prev_blank and not in_list and indent >= 4:
            body = []
            j = i
            while j < n and (not lines[j].strip() or len(lines[j]) - len(lines[j].lstrip(" \t")) >= 4):
                body.append(lines[j])
                j += 1
            while body and not body[-1].strip():
                body.pop()
            blocks.append(Block("code", ln, body, info="indented"))
            i += len(body)
            cur = None
            prev_blank = False
            continue

        setext = SETEXT_RE.match(line) if cur is not None and cur.kind == "paragraph" else None
        if cur is not None and setext is not None:
            ch = setext.group("ch")[0]
            cur.kind = "heading"
            cur.level = 1 if ch == "=" else 2
            cur.lines = [" ".join(s.strip() for s in cur.lines)]
            cur = None
            prev_blank = False
            i += 1
            continue

        if THEMATIC_RE.match(line):
            blocks.append(Block("rule", ln, [line]))
            cur = None
            in_list = False
            prev_blank = False
            i += 1
            continue

        hm = HEADING_RE.match(line)
        if hm:
            htext = (hm.group("text") or "").strip()
            htext = re.sub(r"[ \t]+#+[ \t]*$", "", htext)
            if re.fullmatch(r"#+", htext or ""):
                htext = ""
            blocks.append(Block("heading", ln, [htext], level=len(hm.group("hashes"))))
            cur = None
            in_list = False
            in_table = False
            prev_blank = False
            i += 1
            continue

        if cur is not None and cur.kind == "paragraph" and len(cur.lines) == 1 and "|" in cur.lines[0] \
                and "|" in line and TABLE_DELIM_RE.match(line):
            cur.kind = "table"
            blocks.append(Block("table", ln, [line]))
            cur = None
            in_table = True
            prev_blank = False
            i += 1
            continue

        if line.lstrip().startswith("|") or (in_table and "|" in line):
            blocks.append(Block("table", ln, [line]))
            cur = None
            in_table = True
            prev_blank = False
            i += 1
            continue

        qm = QUOTE_RE.match(line)
        if qm:
            if cur is not None and cur.kind == "quote":
                cur.lines.append(qm.group("rest"))
            else:
                cur = Block("quote", ln, [qm.group("rest")])
                blocks.append(cur)
            prev_blank = False
            i += 1
            continue

        rm = REFDEF_RE.match(line)
        if rm and (cur is None or cur.kind not in ("paragraph", "list")):
            blocks.append(Block("refdef", ln, [line]))
            cur = None
            prev_blank = False
            i += 1
            continue

        if (HTML_ONLY_TAGS_RE.match(line) or HTML_BLOCK_START_RE.match(line)) and (
                cur is None or cur.kind not in ("paragraph", "list")):
            continues = bool(HTML_BLOCK_START_RE.match(line) or HTML_TYPE7_RE.match(line))
            cur = Block("html", ln, [line], info="continues" if continues else "")
            blocks.append(cur)
            prev_blank = False
            i += 1
            continue

        lm = LIST_RE.match(line)
        if lm and not THEMATIC_RE.match(line):
            rest = lm.group("rest") or ""
            rest = re.sub(r"^\[[ xX]\][ \t]+", "", rest)
            cur = Block("list", ln, [rest])
            blocks.append(cur)
            in_list = True
            prev_blank = False
            i += 1
            continue

        if cur is not None and cur.kind in ("paragraph", "list", "quote", "html") and not prev_blank:
            cur.lines.append(line.strip() if cur.kind != "html" else line)
            i += 1
            continue

        if prev_blank and in_list and indent >= 2:
            cur = Block("list", ln, [stripped])
            blocks.append(cur)
            prev_blank = False
            i += 1
            continue

        in_list = False
        cur = Block("paragraph", ln, [stripped])
        blocks.append(cur)
        prev_blank = False
        i += 1

    exempt_level = 0
    for b in blocks:
        if b.kind == "heading":
            title = plain_heading_text(b.lines[0]).strip().lower()
            if exempt_level and b.level <= exempt_level:
                exempt_level = 0
            if title in REFERENCE_SECTION_TITLES:
                exempt_level = b.level
            continue
        b.ste_exempt = exempt_level > 0
    return blocks


# ---------------------------------------------------------------------------
# Inline model
#
# A left-to-right scanner finds inline math spans ($`...`$) and code spans
# (backtick strings of equal length). A link scanner then works on a masked
# copy of the text in which both kinds of span are blanked, so a bracket or a
# URL inside code is never a link. Masking keeps every offset, so all later
# positions map back to source lines.
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    kind: str  # "math" or "code"
    content: str


@dataclass(frozen=True)
class Link:
    start: int  # offset of "[" or "!" or "<"
    end: int
    kind: str  # inline, image, reference, html
    text: str  # link text (inline/image/reference)
    dest: str  # destination, or the reference label for reference links
    dest_start: int
    dest_end: int


def scan_spans(text: str) -> list[Span]:
    spans: list[Span] = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\\" and i + 1 < n and text[i + 1] == "`":
            i += 2
            continue
        if c == "$" and i + 1 < n and text[i + 1] == "`":
            j = text.find("`$", i + 2)
            if j != -1 and "`" not in text[i + 2:j] and j > i + 2:
                spans.append(Span(i, j + 2, "math", text[i + 2:j]))
                i = j + 2
                continue
        if c == "`":
            run = 1
            while i + run < n and text[i + run] == "`":
                run += 1
            k = i + run
            found = -1
            while k < n:
                m = text.find("`" * run, k)
                if m == -1:
                    break
                end = m
                while end < n and text[end] == "`":
                    end += 1
                if end - m == run:
                    found = m
                    break
                k = end
            if found == -1:
                i += run
                continue
            content = text[i + run:found]
            if len(content) >= 2 and content[0] == " " and content[-1] == " " and content.strip():
                content = content[1:-1]
            spans.append(Span(i, found + run, "code", content))
            i = found + run
            continue
        i += 1
    return spans


def mask_spans(text: str, spans: list[Span]) -> str:
    chars = list(text)
    for s in spans:
        for k in range(s.start, s.end):
            if chars[k] != "\n":
                chars[k] = " "
    return "".join(chars)


def _parse_dest(text: str, i: int) -> tuple[str, int, int, int] | None:
    """Parse a link destination and an optional title starting after "(".

    Returns (destination, dest_start, dest_end, index after ")") or None.
    """
    n = len(text)
    while i < n and text[i] in " \t\n":
        i += 1
    if i < n and text[i] == "<":
        j = text.find(">", i + 1)
        if j == -1:
            return None
        dest, ds, de = text[i + 1:j], i + 1, j
        i = j + 1
    else:
        ds = i
        depth = 0
        while i < n:
            c = text[i]
            if c == "\\" and i + 1 < n:
                i += 2
                continue
            if c in " \t\n":
                break
            if c == "(":
                depth += 1
            elif c == ")":
                if depth == 0:
                    break
                depth -= 1
            i += 1
        dest, de = text[ds:i], i
    while i < n and text[i] in " \t\n":
        i += 1
    if i < n and text[i] in "\"'(":
        close = {"\"": "\"", "'": "'", "(": ")"}[text[i]]
        j = text.find(close, i + 1)
        if j == -1:
            return None
        i = j + 1
        while i < n and text[i] in " \t\n":
            i += 1
    if i < n and text[i] == ")":
        return dest, ds, de, i + 1
    return None


def _matching_bracket(text: str, i: int) -> int:
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


HTML_ATTR_LINK_RE = re.compile(
    r"<(?P<tag>[A-Za-z][A-Za-z0-9-]*)\b[^>]*?\b(?P<attr>href|src)\s*=\s*(?P<q>[\"'])(?P<val>[^\"']*)(?P=q)",
    re.IGNORECASE,
)
HTML_TAG_RE = re.compile(r"</?[A-Za-z][A-Za-z0-9-]*(?:\s[^<>]*)?/?>")
HTML_ID_RE = re.compile(r"<[A-Za-z][A-Za-z0-9-]*\b[^>]*?\b(?:id|name)\s*=\s*[\"']([^\"']+)[\"']", re.IGNORECASE)
AUTOLINK_RE = re.compile(r"<[A-Za-z][A-Za-z0-9+.-]{1,31}:[^<>\s]*>")
BARE_URL_RE = re.compile(r"\bhttps?://[^\s<>()\[\]]+(?:\([^\s<>()]*\)[^\s<>()\[\]]*)*")
ENTITY_RE = re.compile(r"&(?:[A-Za-z][A-Za-z0-9]*|#[0-9]+|#[xX][0-9A-Fa-f]+);")


def find_links(masked: str) -> list[Link]:
    links: list[Link] = []
    n = len(masked)
    i = 0
    while i < n:
        c = masked[i]
        if c == "\\":
            i += 2
            continue
        if c == "[":
            close = _matching_bracket(masked, i)
            if close == -1:
                i += 1
                continue
            image = i > 0 and masked[i - 1] == "!"
            start = i - 1 if image else i
            label_text = masked[i + 1:close]
            if close + 1 < n and masked[close + 1] == "(":
                parsed = _parse_dest(masked, close + 2)
                if parsed is not None:
                    dest, ds, de, after = parsed
                    links.append(Link(start, after, "image" if image else "inline", label_text, dest, ds, de))
                    i += 1
                    continue
            if close + 1 < n and masked[close + 1] == "[":
                close2 = masked.find("]", close + 2)
                if close2 != -1:
                    label = masked[close + 2:close2] or label_text
                    if label.strip() and not label.startswith("^"):
                        links.append(Link(start, close2 + 1, "reference", label_text, label, close + 2, close2))
                    i = close2 + 1
                    continue
        i += 1
    for m in HTML_ATTR_LINK_RE.finditer(masked):
        links.append(Link(m.start(), m.end(), "html", m.group("tag").lower(), m.group("val"),
                          m.start("val"), m.end("val")))
    return links


def is_external(dest: str) -> bool:
    return bool(re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", dest)) or dest.startswith("//")


# ---------------------------------------------------------------------------
# Anchors: GitHub heading slugs and explicit ids
# ---------------------------------------------------------------------------


def plain_heading_text(raw: str) -> str:
    """Return the rendered text of a heading, as GitHub computes it."""
    spans = scan_spans(raw)
    out = []
    pos = 0
    for s in spans:
        out.append(raw[pos:s.start])
        out.append(s.content)
        pos = s.end
    out.append(raw[pos:])
    text = "".join(out)
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", "", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\[[^\]]*\]", r"\1", text)
    text = HTML_TAG_RE.sub("", text)
    text = re.sub(r"\*+|~~", "", text)
    text = ENTITY_RE.sub(lambda m: _decode_entity(m.group(0)), text)
    return text.strip()


def _decode_entity(entity: str) -> str:
    import html

    return html.unescape(entity)


def github_slug(heading_text: str) -> str:
    """github-slugger: lower-case, drop every character outside the Unicode
    letter, mark, number and connector-punctuation classes except space and
    hyphen, then turn each space into a hyphen."""
    out = []
    for ch in heading_text.strip().lower():
        if ch == " ":
            out.append("-")
        elif ch == "-":
            out.append("-")
        else:
            cat = unicodedata.category(ch)
            if cat[0] in ("L", "M", "N") or cat == "Pc":
                out.append(ch)
    return "".join(out)


def anchors_of(blocks: list[Block]) -> set[str]:
    anchors: set[str] = set()
    seen: dict[str, int] = {}
    for b in blocks:
        if b.kind == "code":
            continue
        if b.kind == "heading":
            slug = github_slug(plain_heading_text(b.lines[0]))
            if slug in seen:
                seen[slug] += 1
                anchors.add(f"{slug}-{seen[slug]}")
            else:
                seen[slug] = 0
                anchors.add(slug)
        masked = mask_spans(b.text, scan_spans(b.text))
        for m in HTML_ID_RE.finditer(masked):
            anchors.add(m.group(1))
    return anchors


# ---------------------------------------------------------------------------
# Prose extraction and sentences (STE)
# ---------------------------------------------------------------------------

ABBREVIATIONS = frozenset({
    "e.g.", "i.e.", "etc.", "vs.", "cf.", "approx.", "fig.", "figs.", "eq.", "eqs.", "no.", "nos.",
    "sec.", "ch.", "dr.", "mr.", "ms.", "mrs.", "st.", "jr.", "al.", "resp.", "incl.", "viz.",
})
SENTENCE_END_RE = re.compile(r"[.!?]+[\"'”’)\]]*(?=\s|$)")
WORD_RE = re.compile(r"[^\W_]+(?:[-'’.:/][^\W_]+)*")
APOS = "['’]"
CONTRACTION_RE = re.compile(
    r"(?i)\b(?:"
    r"[a-z]+n" + APOS + r"t"
    r"|[a-z]+" + APOS + r"(?:re|ve|ll)"
    r"|i" + APOS + r"m"
    r"|(?:i|you|he|she|it|we|they|that|who|there|what|where|how)" + APOS + r"d"
    r"|(?:it|that|there|here|what|who|where|when|why|how|he|she|let|this|everyone|everybody|"
    r"someone|somebody|nobody|nothing|something)" + APOS + r"s"
    r")\b"
)
IMPERATIVE_VERBS = frozenset("""
abort accept add adjust allow append apply ask assert assign attach avoid bind build bump call
cancel capture change check choose cite clean clear clone close collect commit compare compile
compute configure confirm connect consult convert copy count create declare decode define delete
deploy derive describe detach disable do document download drop edit emit enable encode ensure
enter evaluate examine exclude execute expand export extend extract fetch fill find finish fix
follow freeze generate give go grant ignore import include increment initialize insert inspect
install keep kill launch leave let link load locate measure merge migrate mount move obtain open
pack parse pass paste pick pin place poll prefer prepare print produce protect provide publish
pull push put raise read rebuild receive recover redirect re-run rerun refuse register reject
reload remove rename render reopen repeat replace replay request require reserve reset resolve
restart restore restrict resume retry return reuse revert rewrite run save scan see select send
set show skip specify start stop submit supply swap switch take test transfer treat trigger trim
undo unlink unlock unpack unwrap update upgrade upload use validate verify visit wait walk watch
wrap write
""".split())
CONDITION_RE = re.compile(
    r"^(?:If|When|Before|After|Unless|Until|Once|While|To|For|On|In|At|During|Where)\b[^,]{1,200},\s*"
    r"(?P<verb>[A-Za-z][A-Za-z-]*)"
)
LABEL_RE = re.compile(r"^\s*\S+(?:\s+\S+){0,3}?\s*:")


@dataclass
class Sentence:
    start: int
    end: int
    text: str
    words: int


def build_prose(text: str, spans: list[Span], links: list[Link]) -> str:
    """Return a copy of a block's text in which every non-prose region is
    blanked. A code span, a math span, a quotation, an autolink or a bare URL
    becomes the one-word placeholder "X". Link targets, images, HTML tags and
    entities become spaces. Offsets do not change."""
    chars = list(text)

    def blank(a: int, b: int, placeholder: bool = False) -> None:
        for k in range(a, b):
            if chars[k] != "\n":
                chars[k] = " "
        if placeholder:
            for k in range(a, b):
                if chars[k] != "\n":
                    chars[k] = "X"
                    break

    for s in spans:
        blank(s.start, s.end, placeholder=True)
    for lk in links:
        if lk.kind == "image":
            blank(lk.start, lk.end)
        elif lk.kind == "inline":
            text_end = lk.start + 1 + len(lk.text)
            blank(lk.start, lk.start + 1)
            blank(text_end, lk.end)
        elif lk.kind == "reference":
            text_end = lk.start + 1 + len(lk.text)
            blank(lk.start, lk.start + 1)
            blank(text_end, lk.end)
    masked = "".join(chars)
    for m in AUTOLINK_RE.finditer(masked):
        blank(m.start(), m.end(), placeholder=True)
    masked = "".join(chars)
    for m in HTML_TAG_RE.finditer(masked):
        blank(m.start(), m.end())
    masked = "".join(chars)
    for m in BARE_URL_RE.finditer(masked):
        blank(m.start(), m.end(), placeholder=True)
    masked = "".join(chars)
    for m in ENTITY_RE.finditer(masked):
        blank(m.start(), m.end())
    masked = "".join(chars)
    for m in re.finditer(r"“[^”]*”|\"[^\"]*\"", masked):
        blank(m.start(), m.end(), placeholder=True)
    for k, ch in enumerate(chars):
        if ch in "*~":
            chars[k] = " "
    return "".join(chars)


def split_sentences(prose: str) -> list[Sentence]:
    sentences: list[Sentence] = []
    start = 0
    for m in SENTENCE_END_RE.finditer(prose):
        end = m.end()
        token_start = prose.rfind(" ", 0, m.start() + 1)
        token_start = max(token_start, prose.rfind("\n", 0, m.start() + 1)) + 1
        token = prose[token_start:m.start() + 1].lstrip("(").lower()
        if token in ABBREVIATIONS:
            continue
        nxt = re.match(r"\s*(\S)", prose[end:])
        if nxt is not None and nxt.group(1).islower():
            continue
        sentences.append(_sentence(prose, start, end))
        start = end
    if prose[start:].strip():
        sentences.append(_sentence(prose, start, len(prose)))
    return [s for s in sentences if s.words > 0]


def _sentence(prose: str, a: int, b: int) -> Sentence:
    raw = prose[a:b]
    lead = len(raw) - len(raw.lstrip())
    return Sentence(a + lead, b, raw.strip(), len(WORD_RE.findall(raw)))


NOUN_FOLLOWERS = frozenset({"is", "are", "was", "were", "has", "have", "does"})


def is_procedural(sentence_text: str) -> bool:
    """True when a sentence starts with an imperative verb, or with a
    condition, a comma, and an imperative verb. A first word that a number,
    a "#" reference, or a form of "be" or "have" follows is a noun, as in
    "Check #1 is a race" or "Test suites are ...", so the sentence is
    descriptive."""
    if LABEL_RE.match(sentence_text):
        return False
    words = sentence_text.split()
    if not words:
        return False
    first = re.sub(r"[^A-Za-z-]", "", words[0])
    noun_start = len(words) > 1 and (
        words[1][:1] in "#0123456789" or any(w.lower() in NOUN_FOLLOWERS for w in words[1:3]))
    if first and first[0].isupper() and first.lower() in IMPERATIVE_VERBS and not noun_start:
        return True
    m = CONDITION_RE.match(sentence_text)
    return bool(m and m.group("verb").lower() in IMPERATIVE_VERBS)


# ---------------------------------------------------------------------------
# Rocq declaration index
# ---------------------------------------------------------------------------

DECL_RE = re.compile(
    r"^\s*(?:#\[[^\]]*\]\s*)*"
    r"(?:(?:Local|Global|Polymorphic|Monomorphic|Program|Private|Cumulative|NonCumulative)\s+)*"
    r"(?P<kw>Theorem|Lemma|Definition|Fixpoint|CoFixpoint|Inductive|CoInductive|Variant|Record|"
    r"Structure|Class|Corollary|Proposition|Example|Remark|Fact|Instance|Let|Property)"
    r"\s+(?P<name>[A-Za-z_][A-Za-z0-9_']*)"
)
CTOR_RE = re.compile(r"^\s*(?:\|\s*|with\s+)(?P<name>[A-Za-z_][A-Za-z0-9_']*)\s*:")
ROCQ_CITE_RE = re.compile(
    r"^(?P<path>(?:[A-Za-z0-9_.-]+/)*)(?P<file>[A-Za-z_][A-Za-z0-9_]*\.v)(?::(?P<line>[0-9]+))?$"
)
IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_']*$")
CONNECTOR_RE = re.compile(r"^[\s(\[—–:-]*(?:(?:in|from|of|at|see)\b[\s(\[—–:-]*)*$", re.IGNORECASE)


def strip_rocq_comments(source: str) -> str:
    """Blank (* nested *) comments and string literals, keeping newlines."""
    chars = list(source)
    n = len(chars)
    i = 0
    depth = 0
    in_string = False
    while i < n:
        if in_string:
            if source[i] == '"':
                if i + 1 < n and source[i + 1] == '"':
                    chars[i] = chars[i + 1] = " "
                    i += 2
                    continue
                in_string = False
            if chars[i] != "\n":
                chars[i] = " "
            i += 1
            continue
        if source.startswith("(*", i):
            depth += 1
            chars[i] = chars[i + 1] = " "
            i += 2
            continue
        if depth > 0:
            if source.startswith("*)", i):
                depth -= 1
                chars[i] = chars[i + 1] = " "
                i += 2
                continue
            if chars[i] != "\n":
                chars[i] = " "
            i += 1
            continue
        if source[i] == '"':
            in_string = True
            chars[i] = " "
        i += 1
    return "".join(chars)


def rocq_declarations(path: Path) -> dict[str, list[int]] | None:
    try:
        source = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return None
    index: dict[str, list[int]] = {}
    for number, line in enumerate(strip_rocq_comments(source).split("\n"), start=1):
        m = DECL_RE.match(line)
        if m:
            index.setdefault(m.group("name"), []).append(number)
            if m.group("kw") in ("Inductive", "Variant", "CoInductive") and ":=" in line:
                tail = line.split(":=", 1)[1]
                for ctor in re.findall(r"(?:^|\|)\s*([A-Za-z_][A-Za-z0-9_']*)", tail):
                    index.setdefault(ctor, []).append(number)
            continue
        m = CTOR_RE.match(line)
        if m:
            index.setdefault(m.group("name"), []).append(number)
    return index


# ---------------------------------------------------------------------------
# Corpus rows (snippet-corpus)
# ---------------------------------------------------------------------------


def normalize_ws(text: str) -> str:
    return " ".join(text.split())


def corpus_rows(path: Path) -> tuple[set[str], list[str]]:
    """Return the normalized rows of one corpus file and the problems found."""
    problems: list[str] = []
    rows: set[str] = set()
    if path.suffix == ".json":
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as err:
            return rows, [f"corpus file {path} is not valid JSON: {err}"]
        stack = [data]
        while stack:
            item = stack.pop()
            if isinstance(item, dict):
                if isinstance(item.get("source"), str):
                    rows.add(normalize_ws(item["source"]))
                stack.extend(item.values())
            elif isinstance(item, list):
                stack.extend(item)
    elif path.suffix == ".toml":
        if tomllib is None:
            return rows, ["Python 3.11 or later (tomllib) is required to read TOML corpus files"]
        try:
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as err:
            return rows, [f"corpus file {path} is not valid TOML: {err}"]
        stack = [data]
        while stack:
            item = stack.pop()
            if isinstance(item, dict):
                if isinstance(item.get("source"), str):
                    rows.add(normalize_ws(item["source"]))
                for key in ("file", "path"):
                    if isinstance(item.get(key), str) and item[key].endswith((".rho", ".rhox")):
                        target = (path.parent / item[key]).resolve()
                        try:
                            rows.add(normalize_ws(target.read_text(encoding="utf-8")))
                        except OSError:
                            problems.append(f"manifest {path.name} lists {item[key]}, which does not exist")
                stack.extend(item.values())
            elif isinstance(item, list):
                stack.extend(item)
    else:
        problems.append(f"corpus file {path} has an unsupported format (expected .json or .toml)")
    return rows, problems


# ---------------------------------------------------------------------------
# Lint context
# ---------------------------------------------------------------------------

DOI_URL_RE = re.compile(r"^https?://(?:dx\.)?doi\.org/(?P<doi>.+)$", re.IGNORECASE)
DOI_TEXT_RE = re.compile(r"\bdoi:\s*(?P<doi>[^\s\]<>\"]+)", re.IGNORECASE)
DOI_WELL_FORMED_RE = re.compile(r"^10\.[0-9]{4,9}(?:\.[0-9]+)*/\S+$")
DIAGRAM_SVG_RE = re.compile(r"(?:^|/)diagrams/[^/]+\.svg$")
KEBAB_MD_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*\.md$")
KEBAB_DIR_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
DIAGRAM_SOURCE_NAME_RE = re.compile(r"^[0-9]{2}[a-z]?-[a-z0-9]+(?:-[a-z0-9]+)*\.puml$")
DIAGRAM_RENDERER_FILES = ("render.sh", "fonts.lock")
DIAGRAM_INCLUDE_NAME_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*\.iuml$")
INCLUDE_RE = re.compile(r"^\s*!(?:include|include_once|include_many)\s+(?P<target>.+?)\s*$")
RENDER_FAIL_RE = re.compile(r"^FAIL (?P<file>\S+): (?P<reason>.*)$")


@dataclass
class Config:
    repo_root: Path
    doc_root: Path
    corpora: list[Path]
    theories: Path
    palette: Path
    render_script: Path
    render_check: bool
    only: frozenset[str] | None = None


@dataclass
class Document:
    path: Path
    blocks: list[Block]
    anchors: set[str]
    refdefs: dict[str, str]


class LintContext:
    def __init__(self, cfg: Config) -> None:
        self.cfg = cfg
        self.violations: set[Violation] = set()
        self._docs: dict[Path, Document | None] = {}
        self._decls: dict[Path, dict[str, list[int]] | None] = {}
        self._corpus: tuple[set[str], list[str], list[Path]] | None = None
        self.linted: list[Path] = []
        self.diagram_dirs: set[Path] = set()
        self._rust_citations: rust_citations.Citations | None = None

    # -- utilities ------------------------------------------------------------

    def display(self, path: Path) -> str:
        try:
            return str(path.resolve().relative_to(self.cfg.repo_root.resolve()))
        except ValueError:
            return str(path)

    def report(self, path: Path, line: int, rule: str, detail: str) -> None:
        if self.cfg.only is not None and rule not in self.cfg.only:
            return
        self.violations.add(Violation(self.display(path), line, rule, detail))

    def document(self, path: Path) -> Document | None:
        key = path.resolve()
        if key not in self._docs:
            try:
                text = key.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError):
                self._docs[key] = None
            else:
                blocks = parse_markdown(text)
                refdefs: dict[str, str] = {}
                for b in blocks:
                    if b.kind == "refdef":
                        m = REFDEF_RE.match(b.lines[0])
                        if m:
                            label = " ".join(m.group("label").split()).lower()
                            refdefs.setdefault(label, m.group("dest").strip("<>"))
                self._docs[key] = Document(key, blocks, anchors_of(blocks), refdefs)
        return self._docs[key]

    def declarations(self, path: Path) -> dict[str, list[int]] | None:
        key = path.resolve()
        if key not in self._decls:
            self._decls[key] = rocq_declarations(key)
        return self._decls[key]

    def corpus(self) -> tuple[set[str], list[str], list[Path]]:
        if self._corpus is None:
            rows: set[str] = set()
            problems: list[str] = []
            missing: list[Path] = []
            for corpus_path in self.cfg.corpora:
                if not corpus_path.is_file():
                    missing.append(corpus_path)
                    continue
                r, p = corpus_rows(corpus_path)
                rows |= r
                problems.extend(p)
            self._corpus = (rows, problems, missing)
        return self._corpus

    # -- per-document rules ---------------------------------------------------

    def lint_file(self, path: Path) -> None:
        doc = self.document(path)
        if doc is None:
            self.report(path, 1, "link", "the document cannot be read as UTF-8 text")
            return
        self.linted.append(path.resolve())
        local_diagrams = path.resolve().parent / "diagrams"
        if local_diagrams.is_dir():
            self.diagram_dirs.add(local_diagrams)
        rholang_blocks: list[Block] = []
        for block in doc.blocks:
            if block.kind == "code":
                self.lint_code_block(path, block, rholang_blocks)
                continue
            self.lint_inline_block(path, doc, block)
        if rholang_blocks:
            self.lint_snippets(path, rholang_blocks)

    def lint_code_block(self, path: Path, block: Block, rholang_blocks: list[Block]) -> None:
        if not block.closed:
            self.report(path, block.start, "markdown-fence", "the fenced code block opened here is never closed")
        lang = block.info.split()[0].lower() if block.info.split() else ""
        if lang == "math":
            for k, line in enumerate(block.lines):
                if "$" in line:
                    self.report(path, block.start + k, "math-delimiter",
                                "a math fence is the delimiter; remove the dollar signs inside it")
        elif lang == "mermaid":
            self.check_mermaid(path, block.start, block.text)
        elif lang in ("rholang", "rho"):
            rholang_blocks.append(block)

    def lint_inline_block(self, path: Path, doc: Document, block: Block) -> None:
        text = block.text
        line_starts = [0]
        for k, ch in enumerate(text):
            if ch == "\n":
                line_starts.append(k + 1)

        def line_of(offset: int) -> int:
            return block.start + bisect.bisect_right(line_starts, offset) - 1

        spans = scan_spans(text)
        masked = mask_spans(text, spans)
        links = find_links(masked) if block.kind != "refdef" else []

        if block.kind == "refdef":
            m = REFDEF_RE.match(text)
            if m:
                dest = m.group("dest").strip("<>")
                self.check_destination(path, block.start, dest, image=False)
        protected = [(lk.dest_start, lk.dest_end) for lk in links]
        protected += [(m.start(), m.end()) for m in HTML_TAG_RE.finditer(masked)]
        protected += [(m.start(), m.end()) for m in AUTOLINK_RE.finditer(masked)]
        protected += [(m.start(), m.end()) for m in BARE_URL_RE.finditer(masked)]

        def is_protected(offset: int) -> bool:
            return any(a <= offset < b for a, b in protected)

        # math-delimiter, math-abut, math-undelimited
        dollar_lines: set[int] = set()
        for k, ch in enumerate(masked):
            if ch == "$" and not is_protected(k):
                dollar_lines.add(line_of(k))
        for ln in sorted(dollar_lines):
            self.report(path, ln, "math-delimiter",
                        "bare dollar sign: write inline math as $`...`$, display math as a ```math fence, "
                        "and a literal dollar sign as inline code")
        for s in spans:
            if s.kind == "math" and s.start > 0 and text[s.start - 1].isascii() and text[s.start - 1].isalpha():
                self.report(path, line_of(s.start), "math-abut",
                            f"the letter '{text[s.start - 1]}' abuts the opening delimiter of $`{s.content}`$")
        for m in re.finditer(r"\\[A-Za-z]{2,}", masked):
            if not is_protected(m.start()):
                self.report(path, line_of(m.start()), "math-undelimited",
                            f"LaTeX control word {m.group(0)} outside a math span; delimit it as $`...`$")

        # links, glossary anchors, citation keys, diagram sources
        for lk in links:
            ln = line_of(lk.start)
            if lk.kind == "reference":
                label = " ".join(lk.dest.split()).lower()
                if label not in doc.refdefs:
                    self.report(path, ln, "link", f"reference link [{lk.dest}] has no definition")
                continue
            self.check_destination(path, ln, lk.dest, image=lk.kind == "image" or lk.text == "img")

        # citation-doi
        for lk in links:
            if lk.kind in ("inline", "html"):
                dm = DOI_URL_RE.match(lk.dest)
                if dm:
                    target_doi = urllib.parse.unquote(dm.group("doi"))
                    self.check_doi(path, line_of(lk.start), target_doi)
                    shown = DOI_TEXT_RE.search(lk.text)
                    if shown:
                        shown_doi = _trim_doi(shown.group("doi"))
                        if shown_doi != target_doi:
                            self.report(path, line_of(lk.start), "citation-doi",
                                        f"the link shows doi:{shown_doi} but targets {target_doi}")
        for m in DOI_TEXT_RE.finditer(masked):
            if not is_protected(m.start()):
                self.check_doi(path, line_of(m.start()), _trim_doi(m.group("doi")))

        # rocq-cite
        prose = build_prose(text, spans, links) if block.kind in ("paragraph", "list", "quote") else None
        sentences = split_sentences(prose) if prose is not None else []
        self.check_rocq_citations(path, block, text, spans, sentences, line_of)

        # rust-cite
        self.check_rust_citations(path, text, line_of)

        # STE
        if block.kind in ("paragraph", "list") and not block.ste_exempt and prose is not None:
            self.check_ste(path, block, prose, sentences, line_of)

    def check_destination(self, path: Path, line: int, dest: str, image: bool) -> None:
        dest = dest.strip()
        if not dest or is_external(dest):
            return
        raw_path, _, fragment = dest.partition("#")
        raw_path = urllib.parse.unquote(raw_path)
        fragment = urllib.parse.unquote(fragment)
        if raw_path.startswith("/"):
            target = (self.cfg.repo_root / raw_path.lstrip("/")).resolve()
        elif raw_path:
            target = (path.resolve().parent / raw_path).resolve()
        else:
            target = path.resolve()
        is_glossary = target.name.lower() == "glossary.md" and bool(fragment)
        is_citation = fragment.startswith("ref-")
        rule = "glossary-anchor" if is_glossary else ("citation-key" if is_citation else "link")
        if not target.exists():
            self.report(path, line, rule, f"{dest}: the target {self.display(target)} does not exist")
            return
        if image and DIAGRAM_SVG_RE.search(raw_path) and target.suffix == ".svg":
            source = target.with_suffix(".puml")
            self.diagram_dirs.add(target.parent)
            if not source.is_file():
                self.report(path, line, "diagram-source",
                            f"{raw_path} has no PlantUML source {source.name} beside it")
        if not fragment:
            return
        if target.is_dir():
            self.report(path, line, rule, f"{dest}: a directory has no anchors")
            return
        if target.suffix.lower() in (".md", ".markdown") or target.name.endswith(".md.txt"):
            doc = self.document(target)
            if doc is None or fragment not in doc.anchors:
                if is_citation:
                    detail = f"citation key {fragment} has no reference entry (id=\"{fragment}\") in {self.display(target)}"
                elif is_glossary:
                    detail = f"#{fragment} is not an entry of {self.display(target)}"
                else:
                    detail = f"{dest}: no anchor #{fragment} in {self.display(target)}"
                self.report(path, line, rule, detail)
        elif not re.fullmatch(r"L[0-9]+(?:-L[0-9]+)?", fragment):
            self.report(path, line, rule, f"{dest}: only a Markdown target or a line anchor (#L12) can take a fragment")

    def check_doi(self, path: Path, line: int, doi: str) -> None:
        if not DOI_WELL_FORMED_RE.match(doi):
            self.report(path, line, "citation-doi", f"malformed DOI {doi!r}: expected 10.<registrant>/<suffix>")

    def check_mermaid(self, path: Path, line: int, source: str) -> None:
        kind = mermaid_type(source)
        if kind is None:
            self.report(path, line, "mermaid", "the Mermaid block declares no diagram type")
            return
        if kind in MERMAID_ALLOWLIST:
            return
        hint = MERMAID_EQUIVALENTS.get(kind.lower(), "a PlantUML diagram")
        self.report(path, line, "mermaid",
                    f"Mermaid '{kind}' diagram: PlantUML expresses this type as {hint}; use PlantUML")

    def check_rocq_citations(self, path, block, text, spans, sentences, line_of) -> None:
        for idx, span in enumerate(spans):
            if span.kind != "code":
                continue
            m = ROCQ_CITE_RE.match(span.content.strip())
            if not m:
                continue
            cite = span.content.strip()
            cite_line = line_of(span.start)
            sentence = next((s for s in sentences if s.start <= span.start < s.end), None)
            name = None
            if idx > 0:
                prev = spans[idx - 1]
                same_line = line_of(prev.end - 1) == cite_line
                same_sentence = sentence is not None and prev.start >= sentence.start
                if (same_line or same_sentence) and prev.kind == "code" and IDENT_RE.match(prev.content.strip()):
                    between = text[prev.end:span.start]
                    if m.group("line") or CONNECTOR_RE.match(between):
                        name = prev.content.strip()
            if m.group("path"):
                target = self.cfg.repo_root / m.group("path") / m.group("file")
            else:
                target = self.cfg.theories / m.group("file")
            decls = self.declarations(target)
            if m.group("line"):
                cited_line = int(m.group("line"))
                if name is None:
                    self.report(path, cite_line, "rocq-cite",
                                f"{cite} cited without a preceding declaration name")
                elif decls is None or name not in decls:
                    self.report(path, cite_line, "rocq-cite", f"{name} cited at {cite} but not found")
                elif cited_line not in decls[name]:
                    actual = ", ".join(f"{m.group('file')}:{n}" for n in decls[name])
                    self.report(path, cite_line, "rocq-cite", f"{name} cited at {cite} but declared at {actual}")
            elif name is not None and (decls is None or name not in decls):
                self.report(path, cite_line, "rocq-cite", f"{name} cited in {cite} but not found")

    def check_rust_citations(self, path: Path, text: str, line_of) -> None:
        if self._rust_citations is None:
            self._rust_citations = rust_citations.Citations(self.cfg.repo_root)
        _, problems = self._rust_citations.check_offsets(text)
        for offset, problem in problems:
            self.report(path, line_of(offset), "rust-cite", problem)

    def check_ste(self, path, block, prose, sentences, line_of) -> None:
        for m in re.finditer(";", prose):
            self.report(path, line_of(m.start()), "ste-semicolon", "semicolon in prose: split the sentence")
        for m in CONTRACTION_RE.finditer(prose):
            self.report(path, line_of(m.start()), "ste-contraction", f"contraction '{m.group(0)}' in prose")
        for s in sentences:
            procedural = block.kind == "list" and is_procedural(s.text)
            limit = 20 if procedural else 25
            if s.words > limit:
                rule = "ste-procedure-length" if procedural else "ste-sentence-length"
                kind = "procedural" if procedural else "descriptive"
                self.report(path, line_of(s.start), rule,
                            f"{kind} sentence has {s.words} words (limit {limit}): '{_excerpt(s.text)}'")
        if len(sentences) > 6:
            self.report(path, block.start, "ste-paragraph-length",
                        f"{'list item' if block.kind == 'list' else 'paragraph'} has {len(sentences)} sentences (limit 6)")

    def lint_snippets(self, path: Path, blocks: list[Block]) -> None:
        rows, problems, missing = self.corpus()
        for corpus_path in missing:
            self.report(path, blocks[0].start, "snippet-corpus",
                        f"the listed corpus file {self.display(corpus_path)} does not exist")
        for problem in problems:
            self.report(path, blocks[0].start, "snippet-corpus", problem)
        for b in blocks:
            if normalize_ws(b.text) not in rows:
                self.report(path, b.start, "snippet-corpus",
                            "the rholang block is not a row of any listed corpus file")

    # -- directory rules ----------------------------------------------------

    def reference_universe(self, diagram_dir: Path) -> list[Path]:
        universe = {p.resolve() for p in self.linted}
        for md in diagram_dir.parent.rglob("*.md"):
            if not is_exempt(md, diagram_dir.parent):
                universe.add(md.resolve())
        for md in default_files(self.cfg):
            universe.add(md.resolve())
        return sorted(universe)

    def lint_diagram_dirs(self) -> None:
        for d in sorted(self.diagram_dirs):
            sources = sorted(d.glob("*.puml"))
            mmd = sorted(d.glob("*.mmd"))
            for source in mmd:
                self.check_mermaid(source, 1, source.read_text(encoding="utf-8", errors="replace"))
            if not sources:
                continue
            referenced: set[Path] = set()
            for md in self.reference_universe(d):
                doc = self.document(md)
                if doc is None:
                    continue
                for block in doc.blocks:
                    if block.kind == "code":
                        continue
                    masked = mask_spans(block.text, scan_spans(block.text))
                    for lk in find_links(masked):
                        if lk.kind == "reference" or is_external(lk.dest):
                            continue
                        raw = urllib.parse.unquote(lk.dest.partition("#")[0])
                        if not raw:
                            continue
                        target = (md.parent / raw).resolve()
                        if target.suffix in (".svg", ".puml"):
                            referenced.add(target.with_suffix(".puml"))
            palette = self.cfg.palette.resolve()
            for source in sources:
                if source.resolve() not in referenced:
                    self.report(source, 1, "diagram-orphan",
                                f"{source.name} is neither embedded (as {source.stem}.svg) nor linked by a document")
                self.check_palette(source, palette)
            if self.cfg.render_check:
                self.run_render_check(d)

    def check_palette(self, source: Path, palette: Path) -> None:
        text = source.read_text(encoding="utf-8", errors="replace")
        includes_palette = False
        for line in text.split("\n"):
            m = INCLUDE_RE.match(line)
            if not m:
                continue
            target = m.group("target").strip().strip("\"'")
            if target.startswith("<"):
                continue
            if (source.parent / target).resolve() == palette:
                includes_palette = True
        if not includes_palette:
            self.report(source, 1, "diagram-palette",
                        f"{source.name} does not include the shared palette {self.display(palette)}")
        elif not re.search(r"\$(?:ca|rrs)_[A-Za-z0-9_]+", re.sub(r"(?m)^\s*!include.*$", "", text)):
            self.report(source, 1, "diagram-palette", f"{source.name} includes the palette but uses none of its colors")

    def run_render_check(self, d: Path) -> None:
        script = self.cfg.render_script
        if not script.is_file():
            self.report(d, 1, "diagram-render", f"the render script {self.display(script)} does not exist")
            return
        try:
            proc = subprocess.run(["bash", str(script), "--check", "--dir", str(d)],
                                  capture_output=True, text=True, timeout=1800, check=False)
        except (OSError, subprocess.TimeoutExpired) as err:
            self.report(d, 1, "diagram-render", f"render.sh --check did not finish: {err}")
            return
        reported = False
        for line in proc.stdout.splitlines():
            m = RENDER_FAIL_RE.match(line)
            if m:
                self.report(d / m.group("file"), 1, "diagram-render", m.group("reason"))
                reported = True
        if proc.returncode != 0 and not reported:
            tail = " ".join((proc.stderr or proc.stdout).split())[-400:]
            self.report(d, 1, "diagram-render", f"render.sh --check exited with status {proc.returncode}: {tail}")

    def lint_placement(self) -> None:
        root = self.cfg.doc_root.resolve()
        if not root.is_dir():
            return
        targets = [p for p in self.linted if is_under(p, root)]
        if not targets:
            return
        readme = root / "README.md"
        linked: set[Path] = set()
        if not readme.is_file():
            self.report(root, 1, "placement", "the document root has no README.md index")
        else:
            doc = self.document(readme)
            for block in doc.blocks if doc else []:
                if block.kind == "code":
                    continue
                masked = mask_spans(block.text, scan_spans(block.text))
                for lk in find_links(masked):
                    if lk.kind == "reference" or is_external(lk.dest):
                        continue
                    raw = urllib.parse.unquote(lk.dest.partition("#")[0])
                    if raw:
                        linked.add((readme.parent / raw).resolve())
                for label_dest in (doc.refdefs.values() if doc else []):
                    if not is_external(label_dest):
                        linked.add((readme.parent / urllib.parse.unquote(label_dest.partition("#")[0])).resolve())
        for p in sorted(targets):
            rel = p.relative_to(root)
            for part in rel.parts[:-1]:
                if not KEBAB_DIR_RE.match(part):
                    self.report(p, 1, "placement", f"directory '{part}' is not kebab-case")
            if p.name != "README.md" and not KEBAB_MD_RE.match(p.name):
                self.report(p, 1, "placement", f"'{p.name}' is not a kebab-case Markdown file name")
            if p != readme.resolve() and readme.is_file() and p not in linked:
                self.report(p, 1, "placement", f"{rel} is not linked from the README.md index")
        diagrams = root / "diagrams"
        if diagrams.is_dir():
            for f in sorted(diagrams.iterdir()):
                if f.is_dir():
                    self.report(f, 1, "placement", "diagrams/ holds no subdirectory")
                elif f.suffix == ".puml" and not DIAGRAM_SOURCE_NAME_RE.match(f.name):
                    self.report(f, 1, "placement", f"diagram source '{f.name}' is not NN[a]-kebab-case.puml")
                elif f.suffix == ".iuml" and not DIAGRAM_INCLUDE_NAME_RE.match(f.name):
                    self.report(f, 1, "placement", f"include '{f.name}' is not kebab-case.iuml")
                elif f.suffix == ".svg" and not DIAGRAM_SOURCE_NAME_RE.match(f.stem + ".puml"):
                    self.report(f, 1, "placement", f"diagram '{f.name}' is not NN[a]-kebab-case.svg")
                elif f.suffix not in (".puml", ".iuml", ".svg", ".mmd") and f.name not in DIAGRAM_RENDERER_FILES:
                    self.report(f, 1, "placement", f"unexpected file '{f.name}' in diagrams/")


def _trim_doi(doi: str) -> str:
    doi = doi.rstrip(".,;:")
    while doi.endswith(")") and doi.count(")") > doi.count("("):
        doi = doi[:-1]
    while doi.endswith("]") and doi.count("]") > doi.count("["):
        doi = doi[:-1]
    return doi.rstrip(".,;:")


def _excerpt(text: str, limit: int = 60) -> str:
    flat = " ".join(text.split())
    return flat if len(flat) <= limit else flat[:limit - 1] + "…"


def mermaid_type(source: str) -> str | None:
    lines = source.split("\n")
    i = 0
    if lines and lines[0].strip() == "---":
        i = 1
        while i < len(lines) and lines[i].strip() != "---":
            i += 1
        i += 1
    for line in lines[i:]:
        s = line.strip()
        if not s or s.startswith("%%"):
            continue
        return s.split()[0].rstrip(":;")
    return None


def is_under(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def is_exempt(path: Path, root: Path | None = None) -> bool:
    parts = path.resolve().parts
    if root is not None:
        try:
            parts = path.resolve().relative_to(root.resolve()).parts
        except ValueError:
            pass
    return any(part in EXEMPT_DIR_NAMES for part in parts[:-1])


def default_files(cfg: Config) -> list[Path]:
    files: list[Path] = []
    root = cfg.doc_root
    if root.is_dir():
        files.extend(p for p in sorted(root.rglob("*.md")) if not is_exempt(p, root))
    chapter_dir = cfg.repo_root / DEFAULT_CHAPTER_GLOB[0]
    if chapter_dir.is_dir():
        files.extend(sorted(chapter_dir.glob(DEFAULT_CHAPTER_GLOB[1])))
    return files


# ---------------------------------------------------------------------------
# Command line
# ---------------------------------------------------------------------------


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="doc_lint.py",
        description="Lint Reified RSpaces documents against the pgmcp documentation guidelines.",
    )
    p.add_argument("files", nargs="*", type=Path, help="Markdown files (default: the campaign documents)")
    p.add_argument("--json", action="store_true", help="print a JSON summary as the last line of stdout")
    p.add_argument("--list-rules", action="store_true", help="print the rule catalog and exit")
    p.add_argument("--repo-root", type=Path, default=DEFAULT_REPO_ROOT, help="repository root")
    p.add_argument("--doc-root", type=Path, help="campaign document root (default: docs/rspace/reified-rspaces)")
    p.add_argument("--corpus", type=Path, action="append",
                   help="snippet corpus file (repeatable; default: the two campaign corpora)")
    p.add_argument("--rocq-theories", type=Path, help="Rocq theories directory for bare File.v citations")
    p.add_argument("--palette", type=Path, help="the shared PlantUML palette")
    p.add_argument("--render-script", type=Path, help="the diagram render script with a --check mode")
    p.add_argument("--no-render-check", action="store_true", help="skip the diagram-render rule")
    p.add_argument("--only", help="comma-separated rule names to report (default: all rules)")
    return p


def config_from_args(args: argparse.Namespace) -> Config:
    root = args.repo_root.resolve()

    def rooted(value: Path | None, default: Path) -> Path:
        if value is not None:
            return value if value.is_absolute() else (Path.cwd() / value)
        return root / default

    only = None
    if args.only:
        only = frozenset(r.strip() for r in args.only.split(",") if r.strip())
    corpora = [c if c.is_absolute() else Path.cwd() / c for c in args.corpus] if args.corpus else \
        [root / c for c in DEFAULT_CORPORA]
    return Config(
        repo_root=root,
        doc_root=rooted(args.doc_root, DEFAULT_DOC_ROOT),
        corpora=corpora,
        theories=rooted(args.rocq_theories, DEFAULT_THEORIES),
        palette=rooted(args.palette, DEFAULT_PALETTE),
        render_script=rooted(args.render_script, DEFAULT_RENDER_SCRIPT),
        render_check=not args.no_render_check,
        only=only,
    )


def lint(files: list[Path], cfg: Config) -> list[Violation]:
    ctx = LintContext(cfg)
    for f in files:
        ctx.lint_file(f)
    ctx.lint_diagram_dirs()
    ctx.lint_placement()
    return sorted(ctx.violations)


def run(argv: list[str] | None = None, out=None, err=None) -> tuple[int, list[Violation]]:
    out = out if out is not None else sys.stdout
    err = err if err is not None else sys.stderr
    args = build_parser().parse_args(argv)
    if args.list_rules:
        for name, description in RULES.items():
            print(f"{name}: {description}", file=out)
        return 0, []
    cfg = config_from_args(args)
    if args.only:
        unknown = sorted(cfg.only - set(RULES)) if cfg.only else []
        if unknown:
            print(f"doc_lint.py: unknown rule(s): {', '.join(unknown)}", file=err)
            return 2, []
    if args.files:
        files = []
        for f in args.files:
            path = f if f.is_absolute() else Path.cwd() / f
            if not path.is_file():
                print(f"doc_lint.py: no such file: {f}", file=err)
                return 2, []
            if is_exempt(path, cfg.doc_root if is_under(path, cfg.doc_root) else None):
                print(f"doc_lint.py: skipped exempt file {f}", file=err)
                continue
            files.append(path)
    else:
        files = default_files(cfg)
    violations = lint(files, cfg)
    for v in violations:
        print(v.render(), file=out)
    if args.json:
        counts: dict[str, int] = {}
        for v in violations:
            counts[v.rule] = counts.get(v.rule, 0) + 1
        summary = {
            "tool": "doc_lint",
            "version": 1,
            "ok": not violations,
            "files": [str(Path(os.path.relpath(f, cfg.repo_root))) if is_under(f, cfg.repo_root) else str(f)
                      for f in files],
            "rules": list(RULES) if cfg.only is None else sorted(cfg.only),
            "violation_count": len(violations),
            "counts_by_rule": dict(sorted(counts.items())),
            "violations": [v.as_json() for v in violations],
        }
        print(json.dumps(summary, sort_keys=False, ensure_ascii=False), file=out)
    return (1 if violations else 0), violations


def main() -> int:
    code, _ = run()
    return code


if __name__ == "__main__":
    sys.exit(main())
