"""Unit tests for check-fips-matrix.py, the checker of the FIPS conformance matrix (P10 step S4).

Each test builds a small repository and a small FIPS root in a scratch directory, with one
passing matrix. A negative control changes one part and expects the named failure. The
scratch root is RRS_SCRATCH, or target/verification/reified-rspaces of the repository, so a
test never writes to the source tree or to /tmp.
"""

import copy
import hashlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent
REPO_ROOT = SCRIPTS.parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {filename}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


MATRIX = load("check_fips_matrix", "check-fips-matrix.py")


def scratch_root() -> Path:
    base = os.environ.get("RRS_SCRATCH") or str(REPO_ROOT / "target" / "verification" / "reified-rspaces")
    root = Path(base) / "fips-matrix-tests"
    root.mkdir(parents=True, exist_ok=True)
    return root


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


FIPS_TEXT = "# A FIPS\n\nThe first clause.\n\nThe second clause.\n"
RUST_SOURCE = """pub fn engine() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_returns_one() {
        assert_eq!(super::engine(), 1);
    }

    #[test]
    #[ignore]
    fn engine_ignored() {}

    fn helper() {}
}
"""
HARNESS = """macro_rules! example_tests { ($($name:ident => $example:ident),* $(,)?) => {}; }

example_tests! {
    example_first => FIRST,
}
"""
ROCQ = "Theorem engine_one : 1 = 1.\nProof. reflexivity. Qed.\n"
TLA = "---- MODULE Engine ----\nInvOne == TRUE\n====\n"
REGISTER = "# Register\n\n### <a id=\"decision-1\"></a>Decision 1: one\n\nText.\n\n### <a id=\"i-1\"></a>I-1: internal\n\nText.\n"
DOC = "# Doc\n\n## The engine\n\nText.\n"


def base_matrix(digest: str) -> dict:
    return {
        "schema": MATRIX.SCHEMA,
        "title": "A test matrix",
        "checker": "scripts/verify-reified/check-fips-matrix.py",
        "rendering": "docs/rspace/reified-rspaces/fips-conformance-matrix.md",
        "fips": [{"key": "FX", "title": "A FIPS", "date": "2026-01-01", "path": "approved/fips.md",
                  "sha256": digest, "normative": [[1, 5]], "outside": "None"}],
        "spec_holes": [{"id": "SH-1", "hole": "A hole", "fips": "FX", "lines": [[5, 5]],
                        "resolution": "A resolution.", "rows": ["FX-02"], "decision": ["decision-1"]}],
        "rows": [
            {"id": "FX-01", "fips": "FX", "lines": [[1, 3]], "clause": "The first clause.", "status": "implemented",
             "code": ["src/engine.rs:1-3"],
             "formal": [{"rocq": "Engine.v", "name": "engine_one", "line": 1},
                        {"tla": "formal/tlaplus/reified_rspaces/Engine.tla", "invariant": "InvOne"}],
             "no_model": None,
             "tests": ["src/engine.rs::engine_returns_one", "tests/harness.rs::example_first"],
             "docs": ["docs/doc.md#the-engine"], "decision": ["i-1"], "reason": None, "note": None},
            {"id": "FX-02", "fips": "FX", "lines": [[5, 5]], "clause": "The second clause.", "status": "deviation",
             "code": [], "formal": [], "no_model": None, "tests": [], "docs": [], "decision": ["decision-1"],
             "reason": None, "note": "A note."},
        ],
    }


class Fixture:
    """A repository with one passing matrix, and the FIPS root beside it."""

    def __init__(self, directory: Path) -> None:
        self.repo = directory / "repo"
        self.fips = directory / "fips"
        write(self.fips / "approved" / "fips.md", FIPS_TEXT)
        digest = hashlib.sha256(FIPS_TEXT.encode("utf-8")).hexdigest()
        write(self.repo / "src" / "engine.rs", RUST_SOURCE)
        write(self.repo / "tests" / "harness.rs", HARNESS)
        write(self.repo / "formal" / "rocq" / "reified_rspaces" / "theories" / "Engine.v", ROCQ)
        write(self.repo / "formal" / "tlaplus" / "reified_rspaces" / "Engine.tla", TLA)
        write(self.repo / "docs" / "rspace" / "reified-rspaces" / "decision-register.md", REGISTER)
        write(self.repo / "docs" / "doc.md", DOC)
        self.data = base_matrix(digest)
        self.save()

    @property
    def matrix_path(self) -> Path:
        return self.repo / MATRIX.MATRIX_PATH

    def save(self) -> None:
        write(self.matrix_path, json.dumps(self.data, indent=2) + "\n")

    def run(self, write_md: bool = False) -> tuple[int, str]:
        out = io.StringIO()
        code = MATRIX.run(self.repo, self.fips, self.matrix_path, write_md, out=out)
        return code, out.getvalue()

    def render(self) -> None:
        code, output = self.run(write_md=True)
        if code != MATRIX.EXIT_PASS:
            raise AssertionError(output)


class FipsMatrixTest(unittest.TestCase):
    def setUp(self) -> None:
        self._dir = tempfile.TemporaryDirectory(dir=scratch_root())
        self.fx = Fixture(Path(self._dir.name))

    def tearDown(self) -> None:
        self._dir.cleanup()

    def change(self, mutate) -> tuple[int, str]:
        """Render the passing matrix, apply `mutate` to the data, save it and check it again."""
        self.fx.render()
        data = copy.deepcopy(self.fx.data)
        mutate(data)
        self.fx.data = data
        self.fx.save()
        code, output = self.fx.run(write_md=True)
        return code, output

    def assertFails(self, result, fragment: str) -> None:
        code, output = result
        self.assertEqual(code, MATRIX.EXIT_FAIL, output)
        self.assertIn(fragment, output)

    def test_complete_matrix_passes_and_its_rendering_is_current(self):
        self.fx.render()
        code, output = self.fx.run()
        self.assertEqual(code, MATRIX.EXIT_PASS, output)
        self.assertIn("ROWS=2", output)
        text = (self.fx.repo / self.fx.data["rendering"]).read_text(encoding="utf-8")
        self.assertIn("### FX-01", text)
        self.assertIn("`src/engine.rs:1-3`", text)

    def test_changed_rendering_fails(self):
        self.fx.render()
        rendering = self.fx.repo / self.fx.data["rendering"]
        rendering.write_text(rendering.read_text(encoding="utf-8") + "\nAn edit.\n", encoding="utf-8")
        self.assertFails(self.fx.run(), "is not the rendering of the JSON")

    def test_uncovered_normative_line_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(lines=[[1, 1]])), "the normative lines 3 have no row")

    def test_row_beyond_the_file_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][1].update(lines=[[5, 9]])), "lie inside")

    def test_digest_mismatch_fails(self):
        self.assertFails(self.change(lambda d: d["fips"][0].update(sha256="0" * 64)), "not the pinned")

    def test_missing_fips_file_fails(self):
        self.assertFails(self.change(lambda d: d["fips"][0].update(path="approved/absent.md")), "is missing; pass --fips-root")

    def test_implemented_row_needs_code_tests_and_docs(self):
        def mutate(d):
            d["rows"][0].update(code=[], tests=[], docs=[])
        result = self.change(mutate)
        for fragment in ("needs code", "needs tests", "needs docs"):
            self.assertFails(result, fragment)

    def test_implemented_row_needs_a_formal_cell_or_a_no_model_reason(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(formal=[])), "a formal cell or a no_model reason")

    def test_implemented_row_takes_not_both_formal_and_no_model(self):
        both = {"reason": "A reason.", "link": "docs/doc.md#the-engine"}
        self.assertFails(self.change(lambda d: d["rows"][0].update(no_model=both)), "not both")

    def test_no_model_reason_alone_passes(self):
        reason = {"reason": "A reason.", "link": "docs/doc.md#the-engine"}
        code, output = self.change(lambda d: d["rows"][0].update(formal=[], no_model=reason))
        self.assertEqual(code, MATRIX.EXIT_PASS, output)

    def test_deviation_row_needs_a_decision(self):
        self.assertFails(self.change(lambda d: d["rows"][1].update(decision=[])), "a deviation row cites a decision")

    def test_informative_row_needs_a_reason(self):
        self.assertFails(self.change(lambda d: d["rows"][1].update(status="informative", decision=[])),
                         "an informative row states its reason")

    def test_code_citation_of_a_blank_line_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(code=["src/engine.rs:3-4"])), "blank line")

    def test_code_citation_of_an_unknown_source_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(code=["src/absent.rs:1"])), "matches no Rust source")

    def test_rocq_name_at_a_wrong_line_fails(self):
        wrong = [{"rocq": "Engine.v", "name": "engine_one", "line": 2}]
        self.assertFails(self.change(lambda d: d["rows"][0].update(formal=wrong)), "not at line 2")

    def test_unknown_rocq_name_fails(self):
        wrong = [{"rocq": "Engine.v", "name": "engine_two", "line": 1}]
        self.assertFails(self.change(lambda d: d["rows"][0].update(formal=wrong)), "engine_two is not declared")

    def test_undefined_tla_invariant_fails(self):
        wrong = [{"tla": "formal/tlaplus/reified_rspaces/Engine.tla", "invariant": "InvTwo"}]
        self.assertFails(self.change(lambda d: d["rows"][0].update(formal=wrong)), "InvTwo is not defined")

    def test_missing_test_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(tests=["src/engine.rs::engine_absent"])), "no such test")

    def test_ignored_test_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(tests=["src/engine.rs::engine_ignored"])), "ignored test")

    def test_function_without_a_test_attribute_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(tests=["src/engine.rs::helper"])),
                         "without a test attribute")

    def test_macro_generated_example_test_passes(self):
        code, output = self.change(lambda d: d["rows"][0].update(tests=["tests/harness.rs::example_first"]))
        self.assertEqual(code, MATRIX.EXIT_PASS, output)

    def test_missing_anchor_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][0].update(docs=["docs/doc.md#absent"])), "no anchor #absent")

    def test_unknown_decision_fails(self):
        self.assertFails(self.change(lambda d: d["rows"][1].update(decision=["decision-9"])), "decision-9 is not an anchor")

    def test_spec_hole_with_an_unknown_row_fails(self):
        self.assertFails(self.change(lambda d: d["spec_holes"][0].update(rows=["FX-09"])), "its rows exist")

    def test_unknown_status_fails_the_schema(self):
        self.assertFails(self.change(lambda d: d["rows"][1].update(status="partial")), "(schema)")

    def test_missing_row_key_fails_the_schema(self):
        self.assertFails(self.change(lambda d: d["rows"][1].pop("note")), "(schema)")

    def test_unreadable_matrix_is_an_input_error(self):
        self.fx.matrix_path.write_text("{", encoding="utf-8")
        code, output = self.fx.run()
        self.assertEqual(code, MATRIX.EXIT_USAGE, output)
        self.assertIn("is not valid JSON", output)


if __name__ == "__main__":
    unittest.main()
