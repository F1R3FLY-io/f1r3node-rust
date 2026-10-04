"""Unit tests for check-complete.py, check-footprint.py and the citation rule
of check-model-map.py.

Each checker gets passing fixtures and failing fixtures (negative controls).
Fixtures are built in a temporary directory, so the source tree is never written.
"""

import contextlib
import hashlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = HERE.parent


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {filename}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


COMPLETE = load("check_complete", "check-complete.py")
FOOTPRINT = load("check_footprint", "check-footprint.py")
MODEL_MAP = load("check_model_map", "check-model-map.py")
CITATIONS = load("rust_citations", "rust_citations.py")

CLEAN_SOURCE = """pub trait Host {}
fn work() -> Result<(), Error> {
    let guard = state.lock().expect("state lock");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn works() {
        assert!(true);
        value.unwrap();
    }
}
"""

CLEAN_TEST = """use proptest::test_runner::Config;
fn config() { Config { failure_persistence: None, ..Config::default() }; }
#[test]
fn passes() {}
"""


def complete_policies():
    return {
        "forbidden_non_test": [
            {"pattern": "panic!", "reason": "no panics"},
            {"pattern": ".unwrap()", "reason": "typed errors"},
            {"pattern": "TODO", "reason": "no placeholders"},
        ],
        "forbidden_anywhere": [{"pattern": "write_exclusive", "reason": "M-1"}],
        "allowed_expect": "\\.expect\\(\"[^\"]* lock\"\\)",
        "test_region_start": "^#\\[cfg\\(test\\)\\]\\s*$",
        "phases": {
            "px": {
                "sources": ["src/lib.rs"],
                "entry_points": [{"file": "src/lib.rs", "pattern": "pub trait Host"}],
                "tests": ["tests/props.rs"],
                "property_files": ["tests/props.rs"],
            }
        },
    }


class CompleteChecker(unittest.TestCase):
    def run_case(self, source=CLEAN_SOURCE, test=CLEAN_TEST, mutate=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "tests").mkdir()
            (root / "src/lib.rs").write_text(source, encoding="utf-8")
            (root / "tests/props.rs").write_text(test, encoding="utf-8")
            policies = complete_policies()
            if mutate is not None:
                mutate(policies)
            return COMPLETE.check_phase(root, policies, "px")

    def test_clean_phase_passes(self):
        self.assertEqual(0, self.run_case())

    def test_forbidden_token_in_non_test_code_fails(self):
        self.assertEqual(1, self.run_case(source=CLEAN_SOURCE.replace("Ok(())", "panic!(\"x\")")))

    def test_forbidden_token_in_a_comment_fails(self):
        self.assertEqual(1, self.run_case(source=CLEAN_SOURCE.replace("fn work()", "// TODO later\nfn work()")))

    def test_forbidden_token_in_test_region_passes(self):
        self.assertEqual(0, self.run_case())

    def test_non_lock_expect_fails(self):
        self.assertEqual(1, self.run_case(source=CLEAN_SOURCE.replace("\"state lock\"", "\"state\"")))

    def test_consensus_token_anywhere_fails(self):
        source = CLEAN_SOURCE.replace("fn works()", "fn works() { let write_exclusive = 1; }\n    fn other()")
        self.assertEqual(1, self.run_case(source=source))

    def test_missing_entry_point_fails(self):
        self.assertEqual(1, self.run_case(source=CLEAN_SOURCE.replace("pub trait Host {}", "")))

    def test_tmp_in_test_file_fails(self):
        self.assertEqual(1, self.run_case(test=CLEAN_TEST + 'const P: &str = "/tmp/x";\n'))

    def test_ignored_test_fails(self):
        self.assertEqual(1, self.run_case(test=CLEAN_TEST.replace("#[test]", "#[test]\n#[ignore]")))

    def test_missing_failure_persistence_fails(self):
        self.assertEqual(1, self.run_case(test=CLEAN_TEST.replace("failure_persistence: None, ", "")))

    def test_missing_source_fails(self):
        def drop_source(policies):
            policies["phases"]["px"]["sources"].append("src/absent.rs")

        self.assertEqual(1, self.run_case(mutate=drop_source))


BASE_MOD = "pub mod a;\npub mod b;\n"
BASE_FIELD = "struct S {\n    field: u8,\n}\n"
PROTECTED = "fn protected() {}\n"


def digest(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


class FootprintChecker(unittest.TestCase):
    def run_case(self, mod_text, field_text, protected_text=PROTECTED, extra_file=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src/protected").mkdir(parents=True)
            (root / "src/mod.rs").write_text(mod_text, encoding="utf-8")
            (root / "src/field.rs").write_text(field_text, encoding="utf-8")
            (root / "src/protected/core.rs").write_text(protected_text, encoding="utf-8")
            if extra_file is not None:
                (root / extra_file).write_text("new\n", encoding="utf-8")
            digests = {
                "src/mod.rs": digest(BASE_MOD),
                "src/field.rs": digest(BASE_FIELD),
                "src/protected/core.rs": digest(PROTECTED),
            }
            (root / "digests.json").write_text(json.dumps({"files": digests}), encoding="utf-8")
            policies = {
                "digests": "digests.json",
                "policies": {
                    "base": {"unchanged": ["src/protected/**"], "no_new_files": ["src/protected/**"], "edits": []},
                    "phase": {
                        "extends": "base",
                        "unchanged": ["src/**"],
                        "edits": [
                            {"path": "src/mod.rs", "added_lines": ["pub mod c;"]},
                            {"path": "src/field.rs", "replaced_lines": [["    field: u8,", "    pub(crate) field: u8,"]]},
                        ],
                    },
                },
            }
            return FOOTPRINT.check(root, policies, "phase")

    def test_exact_approved_edits_pass(self):
        self.assertEqual(0, self.run_case(BASE_MOD + "pub mod c;\n", BASE_FIELD.replace("    field", "    pub(crate) field")))

    def test_missing_added_line_fails(self):
        self.assertEqual(1, self.run_case(BASE_MOD, BASE_FIELD.replace("    field", "    pub(crate) field")))

    def test_extra_change_beyond_the_edit_fails(self):
        self.assertEqual(
            1,
            self.run_case(BASE_MOD + "pub mod c;\npub mod d;\n", BASE_FIELD.replace("    field", "    pub(crate) field")),
        )

    def test_protected_file_change_fails(self):
        self.assertEqual(
            1,
            self.run_case(
                BASE_MOD + "pub mod c;\n",
                BASE_FIELD.replace("    field", "    pub(crate) field"),
                protected_text="fn changed() {}\n",
            ),
        )

    def test_new_file_in_protected_directory_fails(self):
        self.assertEqual(
            1,
            self.run_case(
                BASE_MOD + "pub mod c;\n",
                BASE_FIELD.replace("    field", "    pub(crate) field"),
                extra_file="src/protected/new.rs",
            ),
        )


CITED_SOURCE = "fn first() {}\n\nfn second() {\n    work();\n}\n"


class CitationRule(unittest.TestCase):
    """check-model-map.py: Rust line citations in formal files."""

    def run_case(self, theory, aliases=None, extra_sources=()):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for rel in ("rspace++/src/rspace/host.rs", "a/util/mod.rs", *extra_sources):
                (root / rel).parent.mkdir(parents=True, exist_ok=True)
                (root / rel).write_text(CITED_SOURCE, encoding="utf-8")
            (root / "target/debug").mkdir(parents=True)
            (root / "target/debug/host.rs").write_text(CITED_SOURCE, encoding="utf-8")
            (root / "theory.v").write_text(theory, encoding="utf-8")
            report = MODEL_MAP.Report()
            with contextlib.redirect_stdout(io.StringIO()):
                MODEL_MAP.check_citations(["theory.v"], root, report, aliases or {})
            return report

    def test_cited_code_range_passes(self):
        report = self.run_case("(* rs/host.rs:3-5 and a/util/mod.rs:1 *)\n")
        self.assertEqual((1, 0), (report.passed, report.failed))

    def test_build_output_is_not_a_source(self):
        report = self.run_case("(* host.rs:1 *)\n")
        self.assertEqual((1, 0), (report.passed, report.failed))

    def test_range_that_starts_on_a_blank_line_fails(self):
        report = self.run_case("(* rs/host.rs:2-5 *)\n")
        self.assertEqual((0, 1), (report.passed, report.failed))

    def test_range_that_ends_on_a_blank_line_fails(self):
        report = self.run_case("(* rs/host.rs:1-2 *)\n")
        self.assertEqual((0, 1), (report.passed, report.failed))

    def test_range_outside_the_file_fails(self):
        report = self.run_case("(* rs/host.rs:3-9 *)\n")
        self.assertEqual((0, 1), (report.passed, report.failed))

    def test_missing_source_fails(self):
        report = self.run_case("(* rs/absent.rs:1 *)\n")
        self.assertEqual((0, 1), (report.passed, report.failed))

    def test_ambiguous_short_path_fails_without_an_alias(self):
        report = self.run_case("(* util/mod.rs:1 *)\n", extra_sources=("b/util/mod.rs",))
        self.assertEqual((0, 1), (report.passed, report.failed))

    def test_ambiguous_short_path_passes_with_an_alias(self):
        report = self.run_case("(* util/mod.rs:1 *)\n", aliases={"util/mod.rs": "a/util/mod.rs"},
                               extra_sources=("b/util/mod.rs",))
        self.assertEqual((1, 0), (report.passed, report.failed))

    def test_alias_to_a_missing_source_fails(self):
        report = self.run_case("(* util/mod.rs:1 *)\n", aliases={"util/mod.rs": "c/util/mod.rs"})
        self.assertEqual((0, 1), (report.passed, report.failed))

    def test_every_range_of_a_continued_list_is_checked(self):
        report = self.run_case("(* rs/host.rs:1, 3-5 and\n   2 *)\n")
        self.assertEqual((0, 1), (report.passed, report.failed))


class CitationModule(unittest.TestCase):
    """rust_citations.py: the Markdown forms of a citation list."""

    def check(self, text):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "src/host.rs").write_text(CITED_SOURCE, encoding="utf-8")
            return CITATIONS.Citations(root, {}).check(text)

    def test_code_span_list_checks_every_range(self):
        self.assertEqual((2, []), self.check("See `src/host.rs:1`, `3-5`."))

    def test_code_span_list_with_a_blank_range_fails(self):
        count, problems = self.check("See `src/host.rs:1` and `2`.")
        self.assertEqual(2, count)
        self.assertEqual([(1, "src/host.rs:2 starts on a blank line; cite the code lines")], problems)

    def test_a_number_in_later_prose_is_not_a_range(self):
        self.assertEqual((1, []), self.check("See `src/host.rs:1`. Step 2 follows."))

    def test_problem_lines_count_from_the_start_of_the_text(self):
        count, problems = self.check("first\nsecond `src/host.rs:9`\n")
        self.assertEqual((1, [(2, "src/host.rs:9 is outside the file (5 lines)")]), (count, problems))


if __name__ == "__main__":
    unittest.main()
