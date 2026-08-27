#!/usr/bin/env python3
"""Pre-flight checker for .module files.

This is NOT the elaborator. It is a deliberately small re-implementation of
mettail-elab's lexer and of the four checks that are cheap to reproduce
without the theory algebra:

  * lexical well-formedness and brace/paren balance
  * builder-chain ordering (plan 3.2): Types, Exports, Replacements, Terms,
    Equations, Rewrites, in that order within one chain
  * G6 argument use: every name in a term rule's context is referenced
    exactly once between `|-` and `:`
  * G1 undeclared category: every sort and result category is either
    declared in a Types block in this file or inherited from a parameter
  * label uniqueness within a theory declaration
  * G2 collection sorts limited to HashBag, Set, List

It exists so that hand-written module files can be checked before a machine
with the nightly-2026-02-09 toolchain is available. Passing here does not
mean `cargo test -p mettail-elab` passes: the theory algebra, join collisions
and replacement targets are not modelled.

Usage:  python3 check-modules.py FILE.module [FILE.module ...]
"""

import os
import re
import sys

KEYWORDS = {
    "Module", "Theory", "theory", "import", "as", "from", "Empty", "free",
    "let", "in", "if", "then", "subst", "Types", "Exports", "Replacements",
    "Terms", "Equations", "Rewrites",
}
BUILDERS = ["Types", "Exports", "Replacements", "Terms", "Equations", "Rewrites"]
COLLECTIONS = {"HashBag", "Set", "List"}

TOKEN_RE = re.compile(
    r'"(?:[^"\\]|\\.)*"'          # string literal
    r"|\|-|=>|~>|->|==|/\\|\\\\/|\.\.\."
    r"|[A-Za-z_][A-Za-z0-9_]*"
    r"|[{}()\[\],;:.*#^=\\/]"
    r"|\S"
)


def strip_comments(src):
    """Remove {- block -} and -- line comments, preserving line structure."""
    out, i, depth = [], 0, 0
    n = len(src)
    while i < n:
        if src.startswith("{-", i):
            depth += 1
            i += 2
            continue
        if src.startswith("-}", i):
            depth = max(0, depth - 1)
            i += 2
            continue
        if depth:
            out.append("\n" if src[i] == "\n" else " ")
            i += 1
            continue
        if src.startswith("--", i):
            while i < n and src[i] != "\n":
                out.append(" ")
                i += 1
            continue
        if src[i] == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            out.append(src[i:j + 1])
            i = j + 1
            continue
        out.append(src[i])
        i += 1
    return "".join(out)


def lines_of(src):
    return src.split("\n")


class Checker:
    def __init__(self, path, src):
        self.path = path
        self.raw = src
        self.src = strip_comments(src)
        self.errors = []

    def err(self, lineno, msg):
        self.errors.append(f"{self.path}:{lineno}: {msg}")

    # ------------------------------------------------------------ balance
    def check_balance(self):
        pairs = {"}": "{", ")": "(", "]": "["}
        stack = []
        for lineno, line in enumerate(lines_of(self.src), 1):
            for tok in TOKEN_RE.findall(line):
                if tok.startswith('"'):
                    continue
                if tok in "{([":
                    stack.append((tok, lineno))
                elif tok in pairs:
                    if not stack or stack[-1][0] != pairs[tok]:
                        self.err(lineno, f"unbalanced `{tok}`")
                        return
                    stack.pop()
        if stack:
            tok, lineno = stack[-1]
            self.err(lineno, f"unclosed `{tok}`")

    # ------------------------------------------------------ builder order
    def check_builder_order(self):
        """Within one theory body, builders must appear in chain order."""
        depth_of_theory = None
        depth = 0
        last_idx = -1
        for lineno, line in enumerate(lines_of(self.src), 1):
            toks = [t for t in TOKEN_RE.findall(line) if not t.startswith('"')]
            for k, tok in enumerate(toks):
                if tok == "Theory":
                    depth_of_theory = depth
                    last_idx = -1
                elif tok in BUILDERS and k + 1 < len(toks) and toks[k + 1] == "{":
                    idx = BUILDERS.index(tok)
                    if depth_of_theory is not None and idx <= last_idx:
                        self.err(
                            lineno,
                            f"builder `{tok}` follows `{BUILDERS[last_idx]}`; "
                            "the chain is ordered (plan 3.2) and a forward "
                            "reference is a diagnostic",
                        )
                    last_idx = max(last_idx, idx)
                elif tok in "{([":
                    depth += 1
                elif tok in "})]":
                    depth -= 1

    # -------------------------------------------------- collect categories
    def imported_categories(self):
        """Categories reachable through `import "X.module" as a` and
        `import N from "X.module"`. A parameter typed by an imported theory
        brings its categories into scope, so they are not undeclared here."""
        cats = set()
        base = os.path.dirname(os.path.abspath(self.path))
        for m in re.finditer(r'\bimport\b[^\n]*?"([^"]+)"', self.src):
            target = os.path.join(base, m.group(1))
            if not os.path.exists(target):
                self.err(1, f"import target not found: {m.group(1)}")
                continue
            with open(target) as f:
                sub = Checker(target, f.read())
            cats |= sub.own_categories() | sub.imported_categories()
        return cats

    def declared_categories(self):
        return self.own_categories() | self.imported_categories()

    def own_categories(self):
        cats = set()
        for m in re.finditer(r"\b(Types|Exports)\s*\{(.*?)\}", self.src, re.S):
            for entry in m.group(2).split(";"):
                entry = entry.strip()
                if not entry:
                    continue
                # `Elem` or `Elem => Proc`
                parts = [p.strip() for p in entry.split("=>")]
                for p in parts:
                    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", p):
                        cats.add(p)
        return cats

    # --------------------------------------------------------- term rules
    def term_rules(self):
        """Yield (lineno, label, context_src, syntax_src, result)."""
        text = self.src
        offsets = [0]
        for line in text.split("\n"):
            offsets.append(offsets[-1] + len(line) + 1)

        def line_at(pos):
            lo, hi = 0, len(offsets) - 1
            while lo < hi:
                mid = (lo + hi) // 2
                if offsets[mid] <= pos:
                    lo = mid + 1
                else:
                    hi = mid
            return lo

        # Term rules live inside Terms { } and Replacements { } blocks.
        for blk in re.finditer(r"\b(Terms|Replacements)\s*\{", text):
            start = blk.end()
            depth = 1
            i = start
            while i < len(text) and depth:
                if text[i] == "{":
                    depth += 1
                elif text[i] == "}":
                    depth -= 1
                i += 1
            body = text[start:i - 1]
            base = start
            for stmt in self._split_statements(body):
                s, off = stmt
                if "|-" not in s:
                    continue
                lhs, rest = s.split("|-", 1)
                if ":" not in rest:
                    continue
                syntax, result = rest.rsplit(":", 1)
                label_part = lhs.split(".", 1)
                if len(label_part) < 2:
                    continue
                label = label_part[0].strip().split("=>")[-1].strip()
                yield (
                    line_at(base + off),
                    label,
                    label_part[1],
                    syntax,
                    result.strip(),
                )

    @staticmethod
    def _split_statements(body):
        out, depth, start = [], 0, 0
        for i, ch in enumerate(body):
            if ch in "{([":
                depth += 1
            elif ch in "})]":
                depth -= 1
            elif ch == ";" and depth == 0:
                out.append((body[start:i], start))
                start = i + 1
        tail = body[start:]
        if tail.strip():
            out.append((tail, start))
        return out

    def theory_starts(self):
        """Line numbers at which a `Theory` declaration begins. Label
        uniqueness is scoped to one declaration, not to the file: UnivAlg
        legitimately introduces `Zero` in two different theories."""
        starts = []
        for lineno, line in enumerate(lines_of(self.src), 1):
            for tok in TOKEN_RE.findall(line):
                if tok == "Theory":
                    starts.append(lineno)
        return starts

    def check_term_rules(self):
        declared = self.declared_categories()
        starts = self.theory_starts()

        def scope_of(lineno):
            s = 0
            for i, st in enumerate(starts, 1):
                if st <= lineno:
                    s = i
            return s

        seen_labels = {}
        for lineno, label, ctx, syntax, result in self.term_rules():
            key = (scope_of(lineno), label)
            if key in seen_labels:
                self.err(
                    lineno,
                    f"label `{label}` repeated within one theory (first at "
                    f"line {seen_labels[key]})",
                )
            seen_labels[key] = lineno

            names, sorts = self._parse_context(ctx)
            for sort in sorts:
                m = re.fullmatch(r"([A-Za-z_]\w*)\(([A-Za-z_]\w*)\)", sort)
                if m:
                    if m.group(1) not in COLLECTIONS:
                        self.err(
                            lineno,
                            f"unknown collection sort `{m.group(1)}`; expected "
                            f"one of {', '.join(sorted(COLLECTIONS))}",
                        )
                    inner = m.group(2)
                    if declared and inner not in declared:
                        self.err(lineno, f"undeclared category `{inner}`")
                elif declared and sort not in declared:
                    self.err(lineno, f"undeclared category `{sort}`")

            if declared and result not in declared:
                self.err(lineno, f"undeclared result category `{result}`")

            refs = self._syntax_refs(syntax)
            for name in names:
                c = refs.count(name)
                if c != 1:
                    self.err(
                        lineno,
                        f"`{label}`: argument `{name}` referenced {c} times in "
                        "the concrete syntax; G6 requires exactly once",
                    )
            for r in set(refs):
                if r not in names:
                    self.err(
                        lineno,
                        f"`{label}`: `{r}` appears in the concrete syntax but "
                        "is not in the context",
                    )

    @staticmethod
    def _parse_context(ctx):
        """Return (arg_names, sort_strings) for one term-rule context."""
        names, sorts = [], []
        for part in ctx.split(","):
            part = part.strip()
            if not part:
                continue
            m = re.fullmatch(
                r"\^\s*(\w+)\s*\.\s*(\w+)\s*:\s*\[\s*(\w+)\s*->\s*(\w+)\s*\]",
                part,
            )
            if m:
                names.extend([m.group(1), m.group(2)])
                sorts.extend([m.group(3), m.group(4)])
                continue
            m = re.fullmatch(r"(\w+)\s*:\s*([A-Za-z_]\w*(?:\(\w+\))?)", part)
            if m:
                names.append(m.group(1))
                sorts.append(m.group(2))
            else:
                names.append("<unparsed:" + part + ">")
        return names, sorts

    @staticmethod
    def _syntax_refs(syntax):
        """Identifiers between `|-` and `:`, excluding string terminals and
        the `.*sep("x")` projection machinery."""
        syntax = re.sub(r"\.\s*\*\s*sep\s*\([^)]*\)", "", syntax)
        syntax = re.sub(r'"(?:[^"\\]|\\.)*"', " ", syntax)
        return re.findall(r"[A-Za-z_]\w*", syntax)

    def run(self):
        self.check_balance()
        if self.errors:
            return self.errors
        self.check_builder_order()
        self.check_term_rules()
        return self.errors


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    bad = 0
    for path in argv[1:]:
        with open(path) as f:
            src = f.read()
        errs = Checker(path, src).run()
        if errs:
            bad = 1
            for e in errs:
                print(e)
        else:
            print(f"{path}: ok")
    return bad


if __name__ == "__main__":
    sys.exit(main(sys.argv))
