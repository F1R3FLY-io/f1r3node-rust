#!/usr/bin/env python3
"""Check a Reified RSpaces model map (formal/reified-rspaces-model-map*.json).

A model map connects each Rust item of a phase to the Rocq headlines and the
TLA+ invariants that model it. The P1 map (formal/reified-rspaces-model-map.json)
maps the storage substrate to H# and T#, and each P1 fix rule (FR-1 to FR-9,
the round-2 rules FR-1', FR-5', FR-7' and FR-9', and the round-3 rule FR-10)
to its Rust items and headlines. The map of another phase (its "phase" key,
for example P4 in formal/reified-rspaces-model-map-p4.json) defines its own
ids: one letter and a number, such as Q# for Rocq headlines and U# for TLA+
invariants. Its required items are the items that it marks "required", and
its "rules" key is optional. The checker fails when:

  * a required item is missing from the map, or an item covers nothing
    and does not declare why it is unmodeled (an unmapped item).
  * an item cites an H# or T# that the map does not define.
  * a headline names a theorem that the .v file does not define or does not
    check with "Print Assumptions" (an unknown theorem).
  * a T# names an operator that the .tla file does not define (an unknown
    invariant).
  * an item cites a Rocq definition or a TLA+ operator that does not exist.
  * a headline covers no Rust item.
  * a fix rule is missing, names an unknown item or headline, or covers no
    headline without an "unmodeled" reason.
  * (code side) the Rust file of an item is missing, or the item is not in
    it. An item with "status": "pending" is code that another change still
    writes: its absence is a PENDING note, and its presence is a FAIL
    (remove the status). --formal-only skips the code side.
  * (code side) a Rust line citation "path.rs:N", "path.rs:N-M" or a list
    such as "path.rs:N-M, P-Q and R" in a formal file (comments included)
    does not resolve to exactly one Rust source, lies outside the file, or
    starts or ends on a blank line. rust_citations.py defines the rules,
    the "rs/" prefix and the aliases of short paths.

Item kinds: fn (a free fn, or a fn in the impl block of "scope", with any
visibility, const, async or unsafe), struct (a struct or an enum), const (a
free const or static, or an associated const of the impl block of "scope"),
type (a type alias), and variant (a variant of the enum "scope").

A line number in the map that differs from the code is a note, not a failure.
The files of a phase are new and change often.

Exit codes: 0 all checks pass, 1 a check failed, 2 usage or input error,
4 the checks pass but pending items are still absent from the code.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rust_citations  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MAP = REPO_ROOT / "formal" / "reified-rspaces-model-map.json"

# The P1 items that design §5(a) names: TxnCore::stage_*, apply_op in each
# mode, verify, sweep_volatile, sub_order and root_leaf_record_count. The P1
# fix rules add the guards and the typed errors of FR-1 to FR-9. The round-2
# rules remove DuplicateChannelInGroup (FR-1') and add
# begin_evaluation_scope (FR-5'). The round-3 rule FR-10 adds the receipt
# item fired_receipt. Decision 45 (Rule E) retires retention: check_peeks
# replaces peek_retention, retained_results has no caller, and the receipt
# keeps the persist flag of the host for every result.
REQUIRED_ITEMS = (
    "TxnCore::stage_produce",
    "TxnCore::stage_consume",
    "TxnCore::verify",
    "PlayBackend::apply_op",
    "ReplayBackend::apply_op",
    "ReportingRspace::begin",
    "sub_order",
    "sweep_volatile_channels",
    "RSpace::sweep_volatile",
    "ReplayRSpace::sweep_volatile",
    "ReportingRspace::sweep_volatile",
    "SpaceStoreHandle::sweep_volatile",
    "RSpace::root_leaf_record_count",
    "ReplayRSpace::root_leaf_record_count",
    "ReportingRspace::root_leaf_record_count",
    "SpaceStoreHandle::root_leaf_record_count",
    "require_canonical",
    "canonical_group_key",
    "TxnCore::admit_group",
    "channel_key",
    "LockCoverage::require",
    "CommittedParents::admit",
    "TxnRegistration::register",
    "counter_room",
    "SpaceStoreHandle::begin_evaluation_scope",
    # Disabled by decision 45 (Rule E): retained_results is commented out in
    # space_store_handle.rs, because no fire retains a record.
    # "retained_results",
    "check_peeks",
    "fired_receipt",
    "SpaceTxnError::NonCanonicalChannelOrder",
    "SpaceTxnError::UnlockedChannel",
    "SpaceTxnError::ParentOrderReused",
    "SpaceTxnError::CounterOverflow",
    "SpaceTxnError::NestedTransaction",
)
REQUIRED_MODES = ("play", "replay", "reporting")
# The P1 headlines: H1-H22 and T1-T5, and H23 and T6 of decision 36 (replay
# reproduces play).
HEADLINE_IDS = tuple(f"H{i}" for i in range(1, 24)) + tuple(f"T{i}" for i in range(1, 7))
RULE_IDS = tuple(f"FR-{i}" for i in range(1, 11)) + ("FR-1'", "FR-5'", "FR-7'", "FR-9'")
ITEM_KINDS = ("fn", "struct", "const", "type", "variant")
HEADLINE_KINDS = ("rocq", "tla")
# The id of a headline in the map of a phase other than P1: one letter and a
# number.
PHASE_ID = re.compile(r"^[A-Z][0-9]+$")
EXIT_PENDING = 4


def id_key(hid: str) -> tuple[str, int]:
    """Sort ids by their letters, then by their number (Q2 before Q10)."""
    match = re.match(r"^([A-Za-z]+)([0-9]+)$", hid)
    return (match.group(1), int(match.group(2))) if match else (hid, 0)


class Report:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.pending = 0

    def ok(self, message: str) -> None:
        self.passed += 1
        print(f"PASS: {message}")

    def fail(self, message: str) -> None:
        self.failed += 1
        print(f"FAIL: {message}")

    @staticmethod
    def note(message: str) -> None:
        print(f"NOTE: {message}")

    def wait(self, message: str) -> None:
        self.pending += 1
        print(f"PENDING: {message}")


def strip_rocq_comments(text: str) -> str:
    """Remove nested (* ... *) comments. Keep newlines for line numbers."""
    out = []
    depth = 0
    i = 0
    while i < len(text):
        pair = text[i:i + 2]
        if pair == "(*":
            depth += 1
            i += 2
            continue
        if pair == "*)" and depth > 0:
            depth -= 1
            i += 2
            continue
        ch = text[i]
        if depth == 0 or ch == "\n":
            out.append(ch)
        i += 1
    return "".join(out)


def strip_tla_comments(text: str) -> str:
    """Remove nested (* ... *) comments and \\* line comments."""
    text = strip_rocq_comments(text)
    return "\n".join(line.split("\\*", 1)[0] for line in text.split("\n"))


ROCQ_THEOREM = re.compile(
    r"^\s*(?:Theorem|Lemma|Corollary|Proposition|Fact|Remark)\s+([A-Za-z_][A-Za-z0-9_']*)\b",
    re.M)
ROCQ_DEFINITION = re.compile(
    r"^\s*(?:Definition|Fixpoint|CoFixpoint|Inductive|CoInductive|Record|Structure|"
    r"Variant|Let|Notation|Theorem|Lemma|Corollary|Proposition)\s+([A-Za-z_][A-Za-z0-9_']*)\b",
    re.M)
ROCQ_PRINT_ASSUMPTIONS = re.compile(r"^\s*Print\s+Assumptions\s+([A-Za-z_][A-Za-z0-9_'.]*)\s*\.", re.M)
TLA_OPERATOR = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)\s*(?:\([^)]*\))?\s*==", re.M)


def rocq_names(path: Path) -> tuple[set[str], set[str], set[str]]:
    text = strip_rocq_comments(path.read_text(encoding="utf-8"))
    theorems = set(ROCQ_THEOREM.findall(text))
    definitions = set(ROCQ_DEFINITION.findall(text))
    checked = {name.split(".")[-1] for name in ROCQ_PRINT_ASSUMPTIONS.findall(text)}
    return theorems, definitions, checked


def tla_operators(path: Path) -> set[str]:
    text = strip_tla_comments(path.read_text(encoding="utf-8"))
    return set(TLA_OPERATOR.findall(text))


# ── Rust item lookup ────────────────────────────────────────────────────────

IMPL_HEADER = re.compile(r"^impl\b")
# A visibility: pub, pub(crate), pub(super), pub(self) or pub(in path).
VISIBILITY = r"(?:pub(?:\([^)]*\))?\s+)?"
TRAIT_HEADER = re.compile(r"^" + VISIBILITY + r"(?:unsafe\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)")
TOP_ITEM = re.compile(r"^(?:#|//|pub|fn|async|const|static|struct|enum|type|mod|use|macro_rules)")


def strip_generics(text: str) -> str:
    out = []
    depth = 0
    for ch in text:
        if ch == "<":
            depth += 1
        elif ch == ">" and depth > 0:
            depth -= 1
        elif depth == 0:
            out.append(ch)
    return "".join(out)


def impl_self_type(header: str) -> str | None:
    """The self type of an impl header such as 'impl<C> Trait<C> for Type<C>'."""
    rest = strip_generics(header[len("impl"):])
    rest = rest.split("{", 1)[0].split(" where", 1)[0]
    if " for " in rest:
        rest = rest.split(" for ", 1)[1]
    match = re.search(r"([A-Za-z_][A-Za-z0-9_:]*)", rest)
    if match is None:
        return None
    return match.group(1).split("::")[-1]


def find_rust_item(lines: list[str], kind: str, name: str, scope: str | None) -> int | None:
    """Return the 1-based line of the item, or None.

    A scope is the self type of a top-level impl block. A free item (scope
    None) is a top-level fn, struct, const or type.
    """
    fn_pattern = re.compile(
        r"^(\s*)" + VISIBILITY + r"(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+"
        + re.escape(name) + r"\b")
    struct_pattern = re.compile(
        r"^" + VISIBILITY + r"(?:struct|enum)\s+" + re.escape(name) + r"\b")
    const_pattern = re.compile(
        r"^(\s*)" + VISIBILITY + r"(?:const|static)\s+" + re.escape(name) + r"\b")
    type_pattern = re.compile(r"^" + VISIBILITY + r"type\s+" + re.escape(name) + r"\b")
    current_scope: str | None = None
    in_trait = False
    for index, line in enumerate(lines, start=1):
        if IMPL_HEADER.match(line):
            current_scope = impl_self_type(line)
            in_trait = False
            continue
        if TRAIT_HEADER.match(line):
            current_scope = None
            in_trait = True
            continue
        if line and not line[0].isspace() and TOP_ITEM.match(line) and not line.startswith(("#", "//")):
            if not line.startswith("}"):
                current_scope = None
                in_trait = False
        if kind == "fn":
            match = fn_pattern.match(line)
            if match is None:
                continue
            top_level = match.group(1) == ""
            if scope is None and top_level:
                return index
            if scope is not None and not top_level and not in_trait and current_scope == scope:
                return index
        elif kind == "struct":
            if scope is None and struct_pattern.match(line):
                return index
        elif kind == "type":
            if scope is None and type_pattern.match(line):
                return index
        elif kind == "const":
            match = const_pattern.match(line)
            if match is None:
                continue
            top_level = match.group(1) == ""
            if scope is None and top_level:
                return index
            if scope is not None and not top_level and not in_trait and current_scope == scope:
                return index
    return None


def find_rust_variant(lines: list[str], enum_name: str, variant: str) -> int | None:
    """Return the 1-based line of a variant of the top-level enum enum_name."""
    enum_pattern = re.compile(
        r"^(?:pub(?:\([a-z]+\))?\s+)?enum\s+" + re.escape(enum_name) + r"\b")
    variant_pattern = re.compile(r"^\s+" + re.escape(variant) + r"\s*(?:[,({]|$)")
    inside = False
    for index, line in enumerate(lines, start=1):
        if not inside:
            inside = bool(enum_pattern.match(line))
            continue
        if line.startswith("}"):
            inside = False
            continue
        if variant_pattern.match(line):
            return index
    return None


# ── Checks ──────────────────────────────────────────────────────────────────

def check_citations(formal_files: list[str], root: Path, report: Report,
                    aliases: dict[str, str] | None = None) -> None:
    """Check every Rust line citation in the formal files (comments included)."""
    citations = rust_citations.Citations(root, aliases)
    for rel in formal_files:
        path = root / rel
        if not path.is_file():
            continue
        count, problems = citations.check(path.read_text(encoding="utf-8"))
        for line, problem in problems:
            report.fail(f"citation {rel}:{line}: {problem}")
        if not problems:
            report.ok(f"citations: {rel}: {count} Rust line citations name one source and start and end on code")


def load_map(path: Path) -> dict:
    try:
        with path.open(encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        print(f"ERROR: model map not found: {path}", file=sys.stderr)
        sys.exit(2)
    except json.JSONDecodeError as error:
        print(f"ERROR: {path} is not valid JSON: {error}", file=sys.stderr)
        sys.exit(2)
    required_keys = ("schema", "formal_files", "headlines", "items")
    if data.get("phase", "P1") == "P1":
        required_keys += ("rules",)
    for key in required_keys:
        if key not in data:
            print(f"ERROR: {path} has no '{key}' key", file=sys.stderr)
            sys.exit(2)
    if data["schema"] != "reified-rspaces-model-map/2":
        print(f"ERROR: unknown schema {data['schema']!r}", file=sys.stderr)
        sys.exit(2)
    return data


def check(data: dict, formal_only: bool, report: Report) -> None:
    phase = data.get("phase", "P1")
    p1 = phase == "P1"
    if p1:
        headline_ids, required_items = HEADLINE_IDS, REQUIRED_ITEMS
        required_modes, rule_ids = REQUIRED_MODES, RULE_IDS
    else:
        headline_ids = tuple(sorted(data["headlines"], key=id_key))
        required_items = tuple(e.get("item") for e in data["items"] if e.get("required", False))
        required_modes = ()
        rule_ids = tuple(sorted(data.get("rules") or {}))
        for hid in headline_ids:
            if not PHASE_ID.match(hid):
                report.fail(f"{phase}: headline id {hid} is not one letter and a number")
    rocq_files = [REPO_ROOT / p for p in data["formal_files"].get("rocq", [])]
    tla_files = [REPO_ROOT / p for p in data["formal_files"].get("tla", [])]

    theorems: dict[Path, set[str]] = {}
    checked: dict[Path, set[str]] = {}
    definitions: set[str] = set()
    operators: dict[Path, set[str]] = {}
    for path in rocq_files:
        if not path.is_file():
            report.fail(f"Rocq file is missing: {path.relative_to(REPO_ROOT)}")
            continue
        thms, defs, prints = rocq_names(path)
        theorems[path], checked[path] = thms, prints
        definitions |= defs
    for path in tla_files:
        if not path.is_file():
            report.fail(f"TLA+ file is missing: {path.relative_to(REPO_ROOT)}")
            continue
        operators[path] = tla_operators(path)
    all_operators = set().union(*operators.values()) if operators else set()

    # Headlines.
    headlines: dict = data["headlines"]
    for hid in headline_ids:
        if hid not in headlines:
            report.fail(f"{hid} is not defined in the map")
    for hid, entry in sorted(headlines.items(), key=lambda kv: id_key(kv[0])):
        kind, name = entry.get("kind"), entry.get("name")
        path = REPO_ROOT / entry.get("file", "")
        if kind == "rocq":
            if path not in theorems:
                report.fail(f"{hid}: {entry.get('file')} is not a listed Rocq file")
            elif name not in theorems[path]:
                report.fail(f"{hid}: unknown theorem {name} (not defined in {entry['file']})")
            elif name not in checked[path]:
                report.fail(f"{hid}: theorem {name} has no 'Print Assumptions {name}.' line")
            else:
                report.ok(f"{hid} = theorem {name} ({entry['file']})")
        elif kind == "tla":
            if path not in operators:
                report.fail(f"{hid}: {entry.get('file')} is not a listed TLA+ file")
            elif name not in operators[path]:
                report.fail(f"{hid}: unknown invariant {name} (not defined in {entry['file']})")
            else:
                report.ok(f"{hid} = invariant {name} ({entry['file']})")
        else:
            report.fail(f"{hid}: unknown kind {kind!r}")

    # Items.
    items: list = data["items"]
    seen: dict[str, dict] = {}
    for entry in items:
        item = entry.get("item")
        if not item:
            report.fail("an item has no 'item' name")
            continue
        if item in seen:
            report.fail(f"item {item} occurs twice")
        seen[item] = entry

    for item in required_items:
        if item not in seen:
            report.fail(f"unmapped item: required {phase} item {item} is not in the map")
        elif not seen[item].get("required", False):
            report.fail(f"item {item} is required for {phase} but the map marks it optional")
    for item, entry in seen.items():
        if entry.get("required", False) and item not in required_items:
            report.fail(f"item {item} is marked required but {phase} does not require it")
    apply_modes = {e.get("mode") for e in items if e.get("name") in ("apply_op", "begin") and e.get("mode")}
    for mode in required_modes:
        if mode not in apply_modes:
            report.fail(f"unmapped item: no apply_op entry for the {mode} mode")

    covered: set[str] = set()
    for item, entry in seen.items():
        covers = entry.get("covers") or []
        problems = []
        if not covers and not entry.get("unmodeled"):
            problems.append("covers no headline or invariant and declares no 'unmodeled' reason")
        if covers and entry.get("unmodeled"):
            problems.append("covers headlines but also declares an 'unmodeled' reason")
        for hid in covers:
            if hid not in headlines:
                problems.append(f"cites unknown headline {hid}")
        for name in entry.get("rocq", []):
            if name not in definitions:
                problems.append(f"cites unknown Rocq definition {name}")
        for name in entry.get("tla", []):
            if name not in all_operators:
                problems.append(f"cites unknown TLA+ operator {name}")
        if entry.get("kind") not in ITEM_KINDS:
            problems.append(f"has unknown kind {entry.get('kind')!r}")
        if entry.get("kind") == "variant" and not entry.get("scope"):
            problems.append("is a variant without the enum in 'scope'")
        if entry.get("status") not in (None, "pending"):
            problems.append(f"has unknown status {entry.get('status')!r}")
        if problems:
            label = "unmapped item" if not covers else "item"
            report.fail(f"{label} {item}: " + "; ".join(problems))
        elif covers:
            covered |= set(covers)
            report.ok(f"item {item} -> {', '.join(covers)}")
        else:
            report.ok(f"item {item} is unmodeled: {entry['unmodeled']}")

    for hid in headline_ids:
        if hid in headlines and hid not in covered:
            report.fail(f"{hid} ({headlines[hid].get('name')}) covers no Rust item")

    # Fix rules. The P1 map has them. The map of another phase can omit them.
    rules: dict = data.get("rules") or {}
    for rid in rule_ids:
        if rid not in rules:
            report.fail(f"fix rule {rid} is not in the map")
    in_rule: set[str] = set()
    for rid, rule in sorted(rules.items()):
        problems = []
        if rid not in rule_ids:
            problems.append(f"is not a {phase} fix rule")
        names = rule.get("items") or []
        if not names:
            problems.append("names no Rust item")
        for name in names:
            if name not in seen:
                problems.append(f"names unknown item {name}")
        rule_covers = rule.get("covers") or []
        for hid in rule_covers:
            if hid not in headlines:
                problems.append(f"cites unknown headline {hid}")
        if not rule_covers and not rule.get("unmodeled"):
            problems.append("covers no headline and declares no 'unmodeled' reason")
        if problems:
            report.fail(f"fix rule {rid}: " + "; ".join(problems))
        else:
            in_rule |= set(names)
            target = ", ".join(rule_covers) if rule_covers else f"unmodeled ({rule['unmodeled']})"
            report.ok(f"fix rule {rid} -> {len(names)} items, {target}")
    for item in required_items if p1 else ():
        entry = seen.get(item)
        if entry is not None and entry.get("kind") == "variant" and item not in in_rule:
            report.fail(f"required variant {item} is in no fix rule")

    if formal_only:
        report.note("--formal-only: the Rust code side was not checked")
        return

    # Code side.
    formal_files = list(data["formal_files"].get("rocq", [])) + list(data["formal_files"].get("tla", []))
    check_citations(formal_files, REPO_ROOT, report)
    cache: dict[Path, list[str]] = {}
    for item, entry in seen.items():
        path = REPO_ROOT / entry.get("file", "")
        pending = entry.get("status") == "pending"
        if not path.is_file():
            if pending:
                report.wait(f"code: {item}: file {entry.get('file')} does not exist yet")
            else:
                report.fail(f"code: {item}: file is missing: {entry.get('file')}")
            continue
        if path not in cache:
            cache[path] = path.read_text(encoding="utf-8").split("\n")
        kind = entry.get("kind", "fn")
        if kind == "variant":
            line = find_rust_variant(cache[path], entry.get("scope", ""), entry.get("name", ""))
        else:
            line = find_rust_item(cache[path], kind, entry.get("name", ""), entry.get("scope"))
        if line is None:
            scope = entry.get("scope")
            where = (f"enum {scope}" if kind == "variant" else f"impl {scope}") if scope else "the top level"
            message = f"code: {item}: {kind} {entry.get('name')} is not in {where} of {entry['file']}"
            if pending:
                report.wait(message + " (declared pending)")
            else:
                report.fail(message)
            continue
        if pending:
            report.fail(f"code: {item} exists at {entry['file']}:{line}. Remove its 'pending' status")
            continue
        report.ok(f"code: {item} at {entry['file']}:{line}")
        if entry.get("line") not in (None, line):
            report.note(f"{item}: the map cites line {entry.get('line')}, the code has line {line}")


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--map", type=Path, default=DEFAULT_MAP, help="model map JSON (default: %(default)s)")
    parser.add_argument("--formal-only", action="store_true",
                        help="check the formal side only (skip the Rust code side)")
    args = parser.parse_args()
    data = load_map(args.map)
    report = Report()
    check(data, args.formal_only, report)
    print(f"model-map: PASS={report.passed} FAIL={report.failed} PENDING={report.pending}")
    if report.failed:
        return 1
    return EXIT_PENDING if report.pending else 0


if __name__ == "__main__":
    sys.exit(main())
