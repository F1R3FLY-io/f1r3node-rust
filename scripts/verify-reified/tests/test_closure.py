"""Unit tests for check-closure.py and run-checker-tests.py.

Each rule of each subcommand has a passing case and failing cases (negative
controls). A failing case changes one input and asserts that exactly the
targeted rule fails. The fixtures are synthetic and live in a temporary
directory, so a test never writes into the source tree.

The audit tests run a fake audit script through the real harness, so they
need bash and git on PATH. The tests give the checker an explicit
environment, and they never read the HOME of the caller. A fixture names the
system temporary directory only through a placeholder, and the harness never
runs a script that names it.

Run: python3 scripts/verify-reified/run-checker-tests.py test_closure
"""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent
sys.dont_write_bytecode = True


def load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {filename}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


CLOSURE = load("check_closure", "check-closure.py")
RUNNER = load("run_checker_tests", "run-checker-tests.py")

CALLER_PATH = os.environ.get("PATH") or os.defpath
SYNTHETIC_HOME = "/home/example"
HOME_VARIABLE = "CLOSURE_TEST_HOME"
MARK = "✗"
MISMATCH_TWO = f"{MARK} docs/cites.txt:2  bad two"
MISMATCH_FOUR = f"{MARK} docs/cites.txt:4  bad four"

MODEL_MAP = {
    "headlines": {
        "H1": {"kind": "rocq", "name": "first"},
        "H2": {"kind": "rocq", "name": "second"},
        "T1": {"kind": "tla", "name": "Third"},
    },
    "rules": {
        "FR-1": {"items": ["Core::stage", "helper"], "covers": ["H1"]},
        "FR-2": {"items": ["Core::stage", "Report"], "covers": ["H2"]},
        "FR-8": {"items": ["Report"], "covers": [], "unmodeled": "The counts have no store effect."},
        "FR-9": {"items": ["Guard::register"], "covers": [], "unmodeled": "A guard with no store effect."},
        "FR-7": {"items": ["modeled_helper"], "covers": [], "unmodeled": "A reason that the map contradicts."},
    },
    "items": [
        {"item": "Core::stage", "required": True, "covers": ["H1", "H2"]},
        {"item": "helper", "required": False, "covers": ["H2"]},
        {"item": "other", "required": True, "covers": ["T1"]},
        {"item": "Report", "required": False, "covers": [], "unmodeled": "Bookkeeping."},
        {"item": "Guard::register", "required": True, "covers": [], "unmodeled": "A guard."},
        {"item": "modeled_helper", "required": False, "covers": ["H1"]},
    ],
}

PROPERTIES = [
    {"number": 1, "test": "prop_first", "file": "tests/props.rs", "covers": ["H1"]},
    {"number": 2, "test": "prop_second", "file": "tests/props.rs", "covers": ["H2", "T1"]},
]
PROPS_SOURCE = """proptest! {
    #[test]
    fn prop_first(x in 0..4u8) { let _ = x; }

    #[test]
    fn prop_second(x in 0..4u8) { let _ = x; }
}
"""

LIB_SOURCE = """pub trait Host<C> {
    fn begin(&self) -> Result<(), Error>;
}

pub struct Handle<C> {
    host: C,
}
"""

DESIGN_DOC = """# Design

## S1. Purpose

Text.

![Diagram one.](diagrams/01.svg)

## S2. Terms

More text.
"""

REGISTER_DOC = """# Register

### <a id="decision-1"></a>Decision 1: Scope

Bag and PathMap stay out.

### <a id="decision-2"></a>Decision 2: Held

MeTTa is held.

### <a id="decision-14"></a>Decision 14: Audit

The validator entry is dead.
"""

CITES = "good one\nbad two\ngood three\nbad four\n"

FAKE_AUDIT = """#!/usr/bin/env bash
# A fake audit for the harness tests. It reports each "bad" line of
# docs/cites.txt as a mismatch, as the real audit scripts do.
set -u
REPO="$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel 2>/dev/null)"
if [[ -z "${REPO}" ]]; then
    echo "error: not in a git repo" >&2
    exit 2
fi
INDEX="$(mktemp)"
trap 'rm -f "${INDEX}"' EXIT
grep -n '' "${REPO}/docs/cites.txt" > "${INDEX}"
MISMATCHES=0
TOTAL=0
while IFS=: read -r number text; do
    TOTAL=$((TOTAL + 1))
    if [[ "${text}" == bad* ]]; then
        printf '%s %s:%s  %s\\n' "@MARK@" "${REPO}/docs/cites.txt" "${number}" "${text}"
        MISMATCHES=$((MISMATCHES + 1))
    fi
done < "${INDEX}"
@EXTRA@
if (( MISMATCHES == 0 )); then
    exit 0
fi
echo ""
echo "Found ${MISMATCHES} mismatch(es) out of ${TOTAL} cited fake lines."
exit 1
"""

LEDGER_ORIGINAL = f"# Ledger\nThe plan lives at {SYNTHETIC_HOME}/plans/plan.md today.\nEnd of ledger.\n"


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def fake_audit(extra: str = "") -> str:
    return FAKE_AUDIT.replace("@MARK@", MARK).replace("@EXTRA@", extra)


def rows(output: str) -> list[tuple[str, str, str]]:
    found = []
    for line in output.split("\n"):
        for status in ("PASS", "FAIL"):
            prefix = f"{status}: "
            if line.startswith(prefix):
                rule, _, message = line[len(prefix):].partition(": ")
                found.append((status, rule, message))
    return found


def failed_rules(output: str) -> set[str]:
    return {rule for status, rule, _ in rows(output) if status == "FAIL"}


class Result:
    def __init__(self, code: int, out: str, err: str) -> None:
        self.code, self.out, self.err = code, out, err

    @property
    def failed(self) -> set[str]:
        return failed_rules(self.out)


class Fixture:
    """A synthetic repository with a policy that passes every subcommand."""

    def __init__(self, directory: str) -> None:
        self.root = Path(directory) / "repo"
        self.scratch = Path(directory) / "scratch"
        self.scratch.mkdir()
        write(self.root, "formal/model-map.json", json.dumps(MODEL_MAP))
        self.invariant_map = {"schema": "any/9", "phases": {"P1": {"properties": json.loads(json.dumps(PROPERTIES))}}}
        self.write_invariant_map()
        write(self.root, "tests/props.rs", PROPS_SOURCE)
        write(self.root, "src/lib.rs", LIB_SOURCE)
        write(self.root, "docs/design.md", DESIGN_DOC)
        write(self.root, "docs/register.md", REGISTER_DOC)
        write(self.root, "docs/cites.txt", CITES)
        write(self.root, "scripts/audit/fake-audit.sh", fake_audit())
        self.policy = {
            "schema": CLOSURE.SCHEMA,
            "model_map": "formal/model-map.json",
            "invariant_map": "formal/invariant-map.json",
            "invariant_phase": "P1",
            "leaves": {
                "leaf-model": {
                    "model_items": ["Core::stage", "helper"],
                    "optional_model_items": ["helper"],
                    "headlines": ["H1", "H2"],
                    "unmodeled_items": [{"item": "Report", "rule": "FR-8"}],
                    "entry_points": [
                        {"file": "src/lib.rs", "pattern": r"^pub trait Host<C> \{$"},
                        {"file": "src/lib.rs", "pattern": r"^    fn begin\(&self\)"},
                    ],
                    "doc_headings": [{"file": "docs/design.md", "heading": "## S1. Purpose"}],
                    "doc_references": [{"file": "docs/design.md", "text": "![Diagram one.](diagrams/01.svg)"}],
                },
                "leaf-docs": {
                    "doc_anchors": [
                        {"file": "docs/register.md", "anchor": "decision-1", "keywords": ["Bag", "PathMap"]},
                        {"file": "docs/register.md", "anchor": "decision-14", "keywords": ["validator"]},
                    ],
                },
            },
            "audits": {
                "scripts/audit/fake-audit.sh": {
                    "args": ["--quiet"],
                    "expected_exit": 1,
                    "cited_total": 4,
                    "mismatches": [MISMATCH_TWO, MISMATCH_FOUR],
                    "links": ["docs"],
                    "finding": "fixture-finding",
                },
            },
            "ledger_copy": {
                "path": "docs/record/ledger.md",
                "sha256": "",
                "lines": 0,
                "original_sha256": "",
                "token": "[HOME]",
                "token_count": 1,
                "env": HOME_VARIABLE,
                "readme": {"path": "docs/record/README.md", "must_hold": []},
            },
        }
        self.ledger(LEDGER_ORIGINAL)

    def write_invariant_map(self) -> None:
        write(self.root, "formal/invariant-map.json", json.dumps(self.invariant_map))

    def ledger(self, original: str, copy: str | None = None, readme_digest: bool = True) -> None:
        """Write a consistent copy, README and policy for an original ledger."""
        copy = original.replace(SYNTHETIC_HOME, "[HOME]") if copy is None else copy
        copy_sha, original_sha = sha256_text(copy), sha256_text(original)
        lines = copy.count("\n")
        rows_text = f"| Length | {lines} lines |\n| SHA-256 of the original | `{original_sha}` |\n"
        if readme_digest:
            rows_text += f"| SHA-256 of this copy | `{copy_sha}` |\n"
        readme = ("# Record\n\n## Provenance\n\n| Property | Value |\n|---|---|\n" + rows_text
                  + "| Redaction rule | Replace every `/home/<user>` prefix with `[HOME]`. |\n"
                  + "\n## Supersession map\n\n| Ledger topic | Current decision |\n")
        write(self.root, "docs/record/ledger.md", copy)
        write(self.root, "docs/record/README.md", readme)
        spec = self.policy["ledger_copy"]
        spec.update({"sha256": copy_sha, "lines": lines, "original_sha256": original_sha})
        must_hold = [original_sha, f"{lines} lines", "Replace every `/home/<user>` prefix with `[HOME]`.",
                     "## Supersession map"]
        if readme_digest:
            must_hold.insert(0, copy_sha)
        spec["readme"]["must_hold"] = must_hold

    def run(self, *argv: str, environ: dict | None = None) -> Result:
        policy_path = self.root / "policy.json"
        policy_path.write_text(json.dumps(self.policy), encoding="utf-8")
        out, err = io.StringIO(), io.StringIO()
        environment = {"PATH": CALLER_PATH, HOME_VARIABLE: SYNTHETIC_HOME} if environ is None else environ
        code = CLOSURE.main(["--root", str(self.root), "--policies", str(policy_path), *argv],
                            environ=environment, out=out, err=err, scratch_parent=self.scratch)
        return Result(code, out.getvalue(), err.getvalue())


class ClosureCase(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.fixture = Fixture(self.directory.name)

    def assert_fails_only(self, result: Result, *rules: str) -> None:
        self.assertEqual(1, result.code, result.out + result.err)
        self.assertEqual(set(rules), result.failed, result.out)

    def assert_passes(self, result: Result) -> None:
        self.assertEqual(0, result.code, result.out + result.err)
        self.assertEqual(set(), result.failed, result.out)
        self.assertTrue(rows(result.out), "a passing run must print PASS rows")


class PolicyLoading(ClosureCase):
    def test_wrong_schema_is_an_input_error(self) -> None:
        self.fixture.policy["schema"] = "reified-rspaces-closure-policies/0"
        result = self.fixture.run("modeled")
        self.assertEqual(2, result.code)
        self.assertIn("schema", result.err)

    def test_unknown_leaf_key_is_an_input_error(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["doc_heading"] = []
        self.assertEqual(2, self.fixture.run("docs").code)

    def test_optional_item_outside_model_items_is_an_input_error(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["optional_model_items"] = ["other"]
        self.assertEqual(2, self.fixture.run("modeled").code)

    def test_common_options_after_the_subcommand_work(self) -> None:
        policy_path = self.fixture.root / "policy.json"
        policy_path.write_text(json.dumps(self.fixture.policy), encoding="utf-8")
        out = io.StringIO()
        code = CLOSURE.main(["entry-points", "--root", str(self.fixture.root), "--policies", str(policy_path)],
                            environ={"PATH": CALLER_PATH}, out=out, err=io.StringIO())
        self.assertEqual(0, code, out.getvalue())

    def test_repository_policy_is_valid(self) -> None:
        policies = CLOSURE.load_policies(SCRIPTS / "closure-policies.json")
        self.assertEqual(CLOSURE.SCHEMA, policies["schema"])


class ModeledRules(ClosureCase):
    def test_passing_policy_passes(self) -> None:
        result = self.fixture.run("modeled")
        self.assert_passes(result)
        self.assertEqual({"headline", "item", "overlap", "unmodeled"}, {rule for _, rule, _ in rows(result.out)})

    def test_unknown_item_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["model_items"].append("Core::absent")
        self.assert_fails_only(self.fixture.run("modeled"), "item")

    def test_item_not_required_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["optional_model_items"] = []
        self.assert_fails_only(self.fixture.run("modeled"), "item")

    def test_required_item_declared_optional_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["optional_model_items"] = ["helper", "Core::stage"]
        self.assert_fails_only(self.fixture.run("modeled"), "item")

    def test_item_without_overlap_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["headlines"] = ["H1"]
        self.assert_fails_only(self.fixture.run("modeled"), "overlap")

    def test_unknown_headline_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["headlines"].append("H99")
        self.assert_fails_only(self.fixture.run("modeled"), "headline")

    def test_unknown_unmodeled_rule_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["unmodeled_items"] = [{"item": "Report", "rule": "FR-77"}]
        self.assert_fails_only(self.fixture.run("modeled"), "unmodeled")

    def test_unmodeled_rule_without_reason_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["unmodeled_items"] = [{"item": "Report", "rule": "FR-2"}]
        self.assert_fails_only(self.fixture.run("modeled"), "unmodeled")

    def test_unmodeled_rule_that_does_not_list_the_item_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["unmodeled_items"] = [{"item": "Report", "rule": "FR-9"}]
        self.assert_fails_only(self.fixture.run("modeled"), "unmodeled")

    def test_unmodeled_item_that_the_map_models_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["unmodeled_items"] = [{"item": "modeled_helper", "rule": "FR-7"}]
        self.assert_fails_only(self.fixture.run("modeled"), "unmodeled")

    def test_policy_without_model_entries_fails(self) -> None:
        for key in ("model_items", "optional_model_items", "headlines", "unmodeled_items"):
            del self.fixture.policy["leaves"]["leaf-model"][key]
        self.assert_fails_only(self.fixture.run("modeled"), "coverage")


class InvariantRules(ClosureCase):
    def test_passing_policy_passes(self) -> None:
        self.assert_passes(self.fixture.run("invariants"))

    def test_headline_without_a_property_fails(self) -> None:
        self.fixture.invariant_map["phases"]["P1"]["properties"][1]["covers"] = ["T1"]
        self.fixture.write_invariant_map()
        self.assert_fails_only(self.fixture.run("invariants"), "property")

    def test_property_whose_file_lacks_the_test_fails(self) -> None:
        write(self.fixture.root, "tests/props.rs", PROPS_SOURCE.replace("prop_second", "prop_renamed"))
        self.assert_fails_only(self.fixture.run("invariants"), "property")

    def test_test_in_a_comment_does_not_count(self) -> None:
        write(self.fixture.root, "tests/props.rs", PROPS_SOURCE.replace("    fn prop_second", "    // fn prop_second"))
        self.assert_fails_only(self.fixture.run("invariants"), "property")

    def test_property_file_that_does_not_exist_fails(self) -> None:
        self.fixture.invariant_map["phases"]["P1"]["properties"][0]["file"] = "tests/absent.rs"
        self.fixture.write_invariant_map()
        self.assert_fails_only(self.fixture.run("invariants"), "property")

    def test_missing_phase_is_an_input_error(self) -> None:
        self.fixture.policy["invariant_phase"] = "P9"
        self.assertEqual(2, self.fixture.run("invariants").code)


class EntryPointRules(ClosureCase):
    def test_passing_policy_passes(self) -> None:
        self.assert_passes(self.fixture.run("entry-points"))

    def test_missing_pattern_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["entry_points"].append(
            {"file": "src/lib.rs", "pattern": r"^pub struct Absent<C> \{$"})
        self.assert_fails_only(self.fixture.run("entry-points"), "entry-point")

    def test_missing_file_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["entry_points"][0]["file"] = "src/absent.rs"
        self.assert_fails_only(self.fixture.run("entry-points"), "entry-point")

    def test_invalid_regex_is_an_input_error(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["entry_points"][0]["pattern"] = "(unclosed"
        self.assertEqual(2, self.fixture.run("entry-points").code)


class DocRules(ClosureCase):
    def test_passing_policy_passes(self) -> None:
        result = self.fixture.run("docs")
        self.assert_passes(result)
        self.assertEqual({"heading", "reference", "anchor"}, {rule for _, rule, _ in rows(result.out)})

    def test_missing_heading_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["doc_headings"][0]["heading"] = "## S9. Absent"
        self.assert_fails_only(self.fixture.run("docs"), "heading")

    def test_duplicated_heading_fails(self) -> None:
        write(self.fixture.root, "docs/design.md", DESIGN_DOC + "\n## S1. Purpose\n")
        self.assert_fails_only(self.fixture.run("docs"), "heading")

    def test_heading_as_part_of_a_line_fails(self) -> None:
        write(self.fixture.root, "docs/design.md", DESIGN_DOC.replace("## S1. Purpose", "## S1. Purpose and scope"))
        self.assert_fails_only(self.fixture.run("docs"), "heading")

    def test_missing_reference_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-model"]["doc_references"][0]["text"] = "![Absent.](diagrams/09.svg)"
        self.assert_fails_only(self.fixture.run("docs"), "reference")

    def test_missing_keyword_in_an_anchor_section_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-docs"]["doc_anchors"][0]["keywords"].append("Vector")
        self.assert_fails_only(self.fixture.run("docs"), "anchor")

    def test_keyword_after_the_next_anchor_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-docs"]["doc_anchors"][0]["keywords"] = ["MeTTa"]
        self.assert_fails_only(self.fixture.run("docs"), "anchor")

    def test_keyword_match_is_case_sensitive(self) -> None:
        self.fixture.policy["leaves"]["leaf-docs"]["doc_anchors"][0]["keywords"] = ["bag"]
        self.assert_fails_only(self.fixture.run("docs"), "anchor")

    def test_missing_anchor_fails(self) -> None:
        self.fixture.policy["leaves"]["leaf-docs"]["doc_anchors"][0]["anchor"] = "decision-9"
        self.assert_fails_only(self.fixture.run("docs"), "anchor")

    def test_duplicated_anchor_fails(self) -> None:
        write(self.fixture.root, "docs/register.md",
              REGISTER_DOC + '\n### <a id="decision-14"></a>Decision 14 again\n\nThe validator again.\n')
        self.assert_fails_only(self.fixture.run("docs"), "anchor")


class AuditHarness(ClosureCase):
    SCRIPT = "scripts/audit/fake-audit.sh"

    def audit(self, script_text: str | None = None, environ: dict | None = None) -> Result:
        if script_text is not None:
            write(self.fixture.root, self.SCRIPT, script_text)
        result = self.fixture.run("audit", "--script", self.SCRIPT, environ=environ)
        self.assertEqual([], sorted(path.name for path in self.fixture.scratch.iterdir()),
                         "the harness directory must be removed on every path")
        self.assertEqual(CITES, (self.fixture.root / "docs" / "cites.txt").read_text(encoding="utf-8"))
        return result

    def test_fake_audit_passes(self) -> None:
        result = self.audit()
        self.assert_passes(result)
        self.assertEqual({"static-tmp", "static-mktemp", "syntax", "exit", "stderr", "output", "total", "count",
                          "mismatch", "tmpdir", "home", "harness-root", "cleanup"},
                         {rule for _, rule, _ in rows(result.out)})

    def test_caller_environment_does_not_reach_the_script(self) -> None:
        guard = """if [[ -n "${CLOSURE_SENTINEL:-}" || "${GIT_CONFIG_GLOBAL:-}" != /dev/null ]]; then exit 7; fi
if [[ "${GIT_CONFIG_NOSYSTEM:-}" != 1 || "${LC_ALL:-}" != C.UTF-8 ]]; then exit 7; fi
if [[ ! -d "${HOME}" || ! -d "${TMPDIR}" || -n "$(ls -A "${HOME}")" ]]; then exit 7; fi
if [[ -z "${GIT_CEILING_DIRECTORIES:-}" || "${REPO}" != "${GIT_CEILING_DIRECTORIES}"/* ]]; then exit 7; fi"""
        environ = {"PATH": CALLER_PATH, "CLOSURE_SENTINEL": "leaked", "GIT_DIR": "/nonexistent"}
        self.assert_passes(self.audit(fake_audit(guard), environ=environ))

    def test_wrong_exit_code_fails(self) -> None:
        self.fixture.policy["audits"][self.SCRIPT]["expected_exit"] = 0
        self.assert_fails_only(self.audit(), "exit")

    def test_extra_mismatch_line_fails(self) -> None:
        self.fixture.policy["audits"][self.SCRIPT]["mismatches"] = [MISMATCH_TWO]
        self.assert_fails_only(self.audit(), "mismatch")

    def test_missing_mismatch_line_fails(self) -> None:
        self.fixture.policy["audits"][self.SCRIPT]["mismatches"].append(f"{MARK} docs/cites.txt:9  bad nine")
        self.assert_fails_only(self.audit(), "mismatch")

    def test_mismatch_order_does_not_matter(self) -> None:
        self.fixture.policy["audits"][self.SCRIPT]["mismatches"] = [MISMATCH_FOUR, MISMATCH_TWO]
        self.assert_passes(self.audit())

    def test_wrong_citation_total_fails(self) -> None:
        self.fixture.policy["audits"][self.SCRIPT]["cited_total"] = 5
        self.assert_fails_only(self.audit(), "total")

    def test_summary_count_that_differs_from_the_lines_fails(self) -> None:
        self.assert_fails_only(self.audit(fake_audit("MISMATCHES=$((MISMATCHES + 1))")), "count")

    def test_left_over_tmpdir_file_fails(self) -> None:
        self.assert_fails_only(self.audit(fake_audit().replace("trap 'rm -f \"${INDEX}\"' EXIT\n", "")), "tmpdir")

    def test_left_over_home_file_fails(self) -> None:
        self.assert_fails_only(self.audit(fake_audit(': > "${HOME}/stray.txt"')), "home")

    def test_change_under_the_harness_root_fails(self) -> None:
        self.assert_fails_only(self.audit(fake_audit(': > "${REPO}/stray.txt"')), "harness-root")

    def test_hard_coded_system_tmp_path_fails_and_does_not_run(self) -> None:
        extra = f'readonly UNUSED_SCRATCH="{CLOSURE.SYSTEM_TMP}/fake-audit"'
        result = self.audit(fake_audit(extra))
        self.assert_fails_only(result, "static-tmp", "run")
        self.assertNotIn("exit", {rule for _, rule, _ in rows(result.out)})

    def test_process_id_file_name_fails_and_does_not_run(self) -> None:
        self.assert_fails_only(self.audit(fake_audit('readonly UNUSED_NAME="${REPO}/scratch.$$"')), "static-mktemp", "run")

    def test_mktemp_with_a_directory_fails_and_does_not_run(self) -> None:
        text = fake_audit().replace('INDEX="$(mktemp)"', 'INDEX="$(mktemp -p "${REPO}")"')
        self.assert_fails_only(self.audit(text), "static-mktemp", "run")

    def test_syntax_error_fails(self) -> None:
        text = fake_audit().replace("    exit 2\nfi\n", "    exit 2\n", 1)
        self.assert_fails_only(self.audit(text), "syntax")

    def test_stderr_output_fails(self) -> None:
        self.assert_fails_only(self.audit(fake_audit('echo "warning: fixture" >&2')), "stderr")

    def test_unexpected_output_line_fails(self) -> None:
        self.assert_fails_only(self.audit(fake_audit('echo "noise"')), "output")

    def test_unknown_script_is_an_input_error(self) -> None:
        result = self.fixture.run("audit", "--script", "scripts/audit/absent.sh")
        self.assertEqual(2, result.code)

    def test_absolute_script_path_under_the_root_is_accepted(self) -> None:
        result = self.fixture.run("audit", "--script", str(self.fixture.root / self.SCRIPT))
        self.assert_passes(result)
        self.assertEqual([], sorted(path.name for path in self.fixture.scratch.iterdir()))

    def test_script_outside_the_root_is_an_input_error(self) -> None:
        outside = write(self.fixture.scratch.parent, "elsewhere/fake-audit.sh", fake_audit())
        result = self.fixture.run("audit", "--script", str(outside))
        self.assertEqual(2, result.code)
        self.assertIn("must lie under the root", result.err)

    def test_missing_link_root_is_an_input_error(self) -> None:
        self.fixture.policy["audits"][self.SCRIPT]["links"] = ["docs", "absent"]
        self.assertEqual(2, self.fixture.run("audit", "--script", self.SCRIPT).code)

    def test_missing_path_is_an_input_error(self) -> None:
        result = self.fixture.run("audit", "--script", self.SCRIPT, environ={})
        self.assertEqual(2, result.code)
        self.assertIn("PATH is not set", result.err)


class LedgerCopyRules(ClosureCase):
    def assert_no_home(self, result: Result) -> None:
        self.assertNotIn(SYNTHETIC_HOME, result.out + result.err)

    def test_passing_copy_passes(self) -> None:
        result = self.fixture.run("ledger-copy")
        self.assert_passes(result)
        self.assertEqual({"copy-digest", "copy-lines", "token-count", "no-home-path", "inverse-redaction", "readme"},
                         {rule for _, rule, _ in rows(result.out)})
        self.assert_no_home(result)

    def test_copy_digest_mismatch_fails(self) -> None:
        self.fixture.policy["ledger_copy"]["sha256"] = "0" * 64
        self.assert_fails_only(self.fixture.run("ledger-copy"), "copy-digest")

    def test_line_count_mismatch_fails(self) -> None:
        self.fixture.policy["ledger_copy"]["lines"] += 1
        self.assert_fails_only(self.fixture.run("ledger-copy"), "copy-lines")

    def test_token_count_mismatch_fails(self) -> None:
        self.fixture.ledger(LEDGER_ORIGINAL + f"Again at {SYNTHETIC_HOME}/notes.\n")
        self.assert_fails_only(self.fixture.run("ledger-copy"), "token-count")

    def test_home_path_in_the_copy_fails(self) -> None:
        self.fixture.ledger(LEDGER_ORIGINAL + "A second user sits at /home/other/notes.\n")
        result = self.fixture.run("ledger-copy")
        self.assert_fails_only(result, "no-home-path")
        self.assert_no_home(result)

    def test_inverse_redaction_with_another_home_fails(self) -> None:
        result = self.fixture.run("ledger-copy", environ={"PATH": CALLER_PATH, HOME_VARIABLE: "/home/somebody"})
        self.assert_fails_only(result, "inverse-redaction")
        self.assertNotIn("/home/somebody", result.out)

    def test_readme_without_a_required_string_fails(self) -> None:
        self.fixture.policy["ledger_copy"]["readme"]["must_hold"].append("| Absent row |")
        self.assert_fails_only(self.fixture.run("ledger-copy"), "readme")

    def test_missing_copy_fails_and_the_readme_is_still_checked(self) -> None:
        (self.fixture.root / "docs" / "record" / "ledger.md").unlink()
        result = self.fixture.run("ledger-copy")
        self.assert_fails_only(result, "copy-digest")
        self.assertIn(("PASS", "readme"), {(status, rule) for status, rule, _ in rows(result.out)})

    def test_missing_readme_fails_and_the_copy_is_still_checked(self) -> None:
        (self.fixture.root / "docs" / "record" / "README.md").unlink()
        result = self.fixture.run("ledger-copy")
        self.assert_fails_only(result, "readme")
        self.assertIn(("PASS", "inverse-redaction"), {(status, rule) for status, rule, _ in rows(result.out)})

    def test_unset_redaction_home_is_an_input_error(self) -> None:
        result = self.fixture.run("ledger-copy", environ={"PATH": CALLER_PATH})
        self.assertEqual(2, result.code)
        self.assertIn(HOME_VARIABLE, result.err)

    def test_empty_redaction_home_is_an_input_error(self) -> None:
        self.assertEqual(2, self.fixture.run("ledger-copy", environ={"PATH": CALLER_PATH, HOME_VARIABLE: ""}).code)


class LedgerCopyControls(ClosureCase):
    def test_controls_pass(self) -> None:
        result = self.fixture.run("ledger-copy-controls")
        self.assert_passes(result)
        self.assertEqual({"baseline", "control-copy-byte", "control-readme-digest"},
                         {rule for _, rule, _ in rows(result.out)})
        self.assertNotIn(SYNTHETIC_HOME, result.out)

    def test_controls_fail_without_a_passing_baseline(self) -> None:
        self.fixture.policy["ledger_copy"]["lines"] += 1
        self.assert_fails_only(self.fixture.run("ledger-copy-controls"), "baseline")

    def test_readme_without_the_copy_digest_fails_the_readme_control(self) -> None:
        self.fixture.ledger(LEDGER_ORIGINAL, readme_digest=False)
        self.assert_fails_only(self.fixture.run("ledger-copy-controls"), "control-readme-digest")

    def test_copy_control_fails_when_the_copy_rules_ignore_the_copy(self) -> None:
        original = (self.fixture.root / "docs" / "record" / "ledger.md").read_bytes()
        real = CLOSURE.copy_rules
        with mock.patch.object(CLOSURE, "copy_rules", lambda copy, spec, home: real(original, spec, home)):
            result = self.fixture.run("ledger-copy-controls")
        self.assert_fails_only(result, "control-copy-byte")

    def test_readme_control_fails_when_the_readme_rules_ignore_the_readme(self) -> None:
        original = (self.fixture.root / "docs" / "record" / "README.md").read_text(encoding="utf-8")
        real = CLOSURE.readme_rules
        with mock.patch.object(CLOSURE, "readme_rules", lambda readme, spec: real(original, spec)):
            result = self.fixture.run("ledger-copy-controls")
        self.assert_fails_only(result, "control-readme-digest")

    def test_one_byte_change_avoids_the_token(self) -> None:
        copy = b"[HOME][HOME]a"
        self.assertEqual(12, CLOSURE.one_byte_offset(copy, b"[HOME]"))
        self.assertIsNone(CLOSURE.one_byte_offset(b"[HOME]", b"[HOME]"))

    def test_unset_redaction_home_is_an_input_error(self) -> None:
        self.assertEqual(2, self.fixture.run("ledger-copy-controls", environ={"PATH": CALLER_PATH}).code)


PASSING_MODULE = """import unittest


class Passing(unittest.TestCase):
    def test_passes(self):
        self.assertEqual(2, 1 + 1)
"""
FAILING_MODULE = PASSING_MODULE.replace("2, 1 + 1", "3, 1 + 1")
SKIPPING_MODULE = PASSING_MODULE.replace("    def test_passes", "    @unittest.skip('fixture')\n    def test_passes")
EMPTY_MODULE = "VALUE = 1\n"
BROKEN_MODULE = "raise RuntimeError('the fixture module fails when it is loaded')\n"


class CheckerTestRunner(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.tests_dir = Path(directory.name)
        self.cwd = Path(directory.name)

    def run_fixture(self, **modules: str) -> tuple[int, str]:
        for name, text in modules.items():
            write(self.tests_dir, f"{name}.py", text)
        stream = io.StringIO()
        saved = sys.dont_write_bytecode
        sys.dont_write_bytecode = False
        try:
            code = RUNNER.run_modules(list(modules), tests_dir=self.tests_dir, stream=stream)
        finally:
            sys.dont_write_bytecode = saved
        self.assertFalse((self.tests_dir / "__pycache__").exists(), "the runner must write no bytecode")
        return code, stream.getvalue()

    def run_cli(self, *arguments: str) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, str(SCRIPTS / "run-checker-tests.py"), *arguments],
                              env={"PATH": CALLER_PATH}, cwd=self.cwd, capture_output=True, text=True,
                              timeout=120, check=False)

    def test_passing_module_passes(self) -> None:
        code, output = self.run_fixture(closure_runner_passing=PASSING_MODULE)
        self.assertEqual(0, code, output)
        self.assertIn("PASS: closure_runner_passing: 1 tests, 0 failures, 0 errors, 0 skipped", output)

    def test_failing_module_fails(self) -> None:
        code, output = self.run_fixture(closure_runner_passing_too=PASSING_MODULE, closure_runner_failing=FAILING_MODULE)
        self.assertEqual(1, code, output)
        self.assertIn("FAIL: closure_runner_failing:", output)

    def test_missing_module_fails(self) -> None:
        stream = io.StringIO()
        code = RUNNER.run_modules(["closure_runner_absent"], tests_dir=self.tests_dir, stream=stream)
        self.assertEqual(1, code, stream.getvalue())
        self.assertIn("missing=1", stream.getvalue())

    def test_module_without_tests_fails(self) -> None:
        code, output = self.run_fixture(closure_runner_empty=EMPTY_MODULE)
        self.assertEqual(1, code, output)
        self.assertIn("empty=1", output)

    def test_module_that_fails_to_load_fails(self) -> None:
        code, output = self.run_fixture(closure_runner_broken=BROKEN_MODULE)
        self.assertEqual(1, code, output)
        self.assertIn("errors=1", output)

    def test_skipped_test_fails(self) -> None:
        code, output = self.run_fixture(closure_runner_skipping=SKIPPING_MODULE)
        self.assertEqual(1, code, output)
        self.assertIn("skipped=1", output)

    def test_cli_without_a_module_is_a_usage_error(self) -> None:
        self.assertEqual(2, self.run_cli().returncode)

    def test_cli_with_a_file_name_is_a_usage_error(self) -> None:
        self.assertEqual(2, self.run_cli("test_closure.py").returncode)

    def test_cli_with_an_option_is_a_usage_error(self) -> None:
        self.assertEqual(2, self.run_cli("-m", "unittest").returncode)

    def test_cli_with_a_repeated_module_is_a_usage_error(self) -> None:
        self.assertEqual(2, self.run_cli("closure_runner_absent", "closure_runner_absent").returncode)

    def test_cli_with_a_missing_module_fails(self) -> None:
        completed = self.run_cli("closure_runner_absent")
        self.assertEqual(1, completed.returncode, completed.stdout + completed.stderr)


if __name__ == "__main__":
    unittest.main()
