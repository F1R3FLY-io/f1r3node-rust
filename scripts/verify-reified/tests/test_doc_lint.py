"""Unit tests for scripts/verify-reified/doc_lint.py.

Run from the repository root:

    python3 -m unittest discover -s scripts/verify-reified/tests

Every rule of ``doc_lint.RULES`` has a passing fixture and a failing fixture
under ``fixtures/doc_lint/<rule>/{pass,fail}/``. A failing fixture is a
negative control: the test asserts that the linter reports exactly that rule,
by name, and nothing else. Fixture documents use the extension ``.md.txt``,
so repository-wide Markdown tools (for example the CI link checker, which
scans ``./**/*.md``) never read their deliberate defects.

The diagram-render tests run PlantUML through render.sh. They copy a case into
``${RRS_SCRATCH:-<repo>/target/verification/reified-rspaces}/doc-lint-tests``,
because a test never writes into the source tree and never uses /tmp. These
tests fail, and never skip, when Java or PlantUML is missing.
"""

from __future__ import annotations

import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
VERIFY_DIR = HERE.parent
REPO_ROOT = VERIFY_DIR.parents[1]
SCRIPT = VERIFY_DIR / "doc_lint.py"
FIXTURES = HERE / "fixtures" / "doc_lint"
RENDER_SH = REPO_ROOT / "docs" / "rspace" / "reified-rspaces" / "diagrams" / "render.sh"


def _load_doc_lint():
    spec = importlib.util.spec_from_file_location("doc_lint", SCRIPT)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load doc_lint from {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["doc_lint"] = module
    spec.loader.exec_module(module)
    return module


doc_lint = _load_doc_lint()


def scratch_root() -> Path:
    base = os.environ.get("RRS_SCRATCH") or str(REPO_ROOT / "target" / "verification" / "reified-rspaces")
    root = Path(base) / "doc-lint-tests"
    root.mkdir(parents=True, exist_ok=True)
    return root


def lint_case(case: Path, render: bool = False, extra: tuple[str, ...] = ()):
    """Lint one fixture case. The case directory acts as the repository root."""
    case = Path(case)
    args = [
        "--repo-root", str(case),
        "--rocq-theories", str(case / "theories"),
        "--render-script", str(RENDER_SH),
    ]
    if (case / "palette.iuml").exists():
        args += ["--palette", str(case / "palette.iuml")]
    if (case / "docroot").is_dir():
        args += ["--doc-root", str(case / "docroot")]
        files = sorted((case / "docroot").rglob("*.md"))
    else:
        files = sorted(case.glob("*.md.txt"))
    if not render:
        args.append("--no-render-check")
    out, err = io.StringIO(), io.StringIO()
    code, violations = doc_lint.run(args + list(extra) + [str(f) for f in files], out=out, err=err)
    return code, violations, out.getvalue(), err.getvalue()


def rendered(violations) -> list[str]:
    return [v.render() for v in violations]


# ---------------------------------------------------------------------------
# One passing and one failing fixture per rule
# ---------------------------------------------------------------------------

RENDER_RULE = "diagram-render"
STATIC_RULES = [rule for rule in doc_lint.RULES if rule != RENDER_RULE]


class FixtureCoverage(unittest.TestCase):
    def test_every_rule_has_a_passing_and_a_failing_fixture(self):
        for rule in doc_lint.RULES:
            with self.subTest(rule=rule):
                self.assertTrue((FIXTURES / rule / "pass").is_dir(), f"{rule}: no pass fixture")
                self.assertTrue((FIXTURES / rule / "fail").is_dir(), f"{rule}: no fail fixture")

    def test_fixture_documents_are_not_markdown_files(self):
        # Deliberately broken documents must not end in .md, or the CI link
        # checker would read them. Support documents (targets, indexes) may be
        # .md, because their links resolve.
        for path in FIXTURES.rglob("*.md.txt"):
            self.assertTrue(path.name.endswith(".md.txt"))
        for path in FIXTURES.rglob("*.md"):
            _, violations, _, _ = lint_case_single(path)
            self.assertNotIn("link", {v.rule for v in violations}, rendered(violations))

    def test_no_rholang_source_files_in_fixtures(self):
        # The ordinary source-normalization gate walks every .rho and .rhox
        # file of the workspace, so fixtures must not add one.
        self.assertEqual([], sorted(str(p) for p in FIXTURES.rglob("*.rho")))
        self.assertEqual([], sorted(str(p) for p in FIXTURES.rglob("*.rhox")))


def lint_case_single(path: Path):
    out, err = io.StringIO(), io.StringIO()
    code, violations = doc_lint.run(
        ["--repo-root", str(path.parent), "--no-render-check", "--only", "link", str(path)], out=out, err=err)
    return code, violations, out.getvalue(), err.getvalue()


class StaticRuleFixtures(unittest.TestCase):
    """Generated: test_<rule>_pass and test_<rule>_fail for every static rule."""


def _make_rule_tests(rule: str):
    def test_pass(self):
        code, violations, _, _ = lint_case(FIXTURES / rule / "pass")
        self.assertEqual([], rendered(violations))
        self.assertEqual(0, code)

    def test_fail(self):
        code, violations, _, _ = lint_case(FIXTURES / rule / "fail")
        self.assertTrue(violations, f"{rule}: the negative control reported nothing")
        self.assertEqual({rule}, {v.rule for v in violations}, rendered(violations))
        self.assertEqual(1, code)

    return test_pass, test_fail


for _rule in STATIC_RULES:
    _pass, _fail = _make_rule_tests(_rule)
    _name = _rule.replace("-", "_")
    _pass.__name__ = f"test_{_name}_pass"
    _fail.__name__ = f"test_{_name}_fail"
    setattr(StaticRuleFixtures, _pass.__name__, _pass)
    setattr(StaticRuleFixtures, _fail.__name__, _fail)


# ---------------------------------------------------------------------------
# Exact findings of selected negative controls
# ---------------------------------------------------------------------------


class ExactFindings(unittest.TestCase):
    def findings(self, rule: str, case: str = "fail"):
        _, violations, _, _ = lint_case(FIXTURES / rule / case)
        return [(v.line, v.rule, v.detail) for v in violations]

    def test_math_delimiter_reports_every_dollar_line(self):
        self.assertEqual([3, 5, 7], [line for line, _, _ in self.findings("math-delimiter")])

    def test_math_abut_names_the_letter(self):
        self.assertIn("the letter 'x' abuts", self.findings("math-abut")[0][2])

    def test_link_reports_file_anchor_and_reference(self):
        details = " | ".join(d for _, _, d in self.findings("link"))
        self.assertIn("missing.md: the target missing.md does not exist", details)
        self.assertIn("no anchor #no-such-section in target.md", details)
        self.assertIn("reference link [no-such-label] has no definition", details)

    def test_glossary_anchor_detail(self):
        self.assertEqual([(3, "glossary-anchor", "#no-such-term is not an entry of glossary.md")],
                         self.findings("glossary-anchor"))

    def test_rust_cite_details(self):
        self.assertEqual(
            [
                (3, "rust-cite", "src/rust_cite_fixture.rs:2-5 starts on a blank line; cite the code lines"),
                (4, "rust-cite", "src/absent_fixture.rs matches no Rust source"),
                (5, "rust-cite", "src/rust_cite_fixture.rs:9 is outside the file (5 lines)"),
            ],
            self.findings("rust-cite"),
        )

    def test_rocq_cite_messages_follow_the_audit_format(self):
        self.assertEqual(
            [
                (3, "rocq-cite", "sample_value_is_three cited at Sample.v:2 but declared at Sample.v:4"),
                (4, "rocq-cite", "no_such_lemma cited at Sample.v:4 but not found"),
                (5, "rocq-cite", "hidden_name cited in Sample.v but not found"),
                (6, "rocq-cite", "Sample.v:4 cited without a preceding declaration name"),
            ],
            self.findings("rocq-cite"),
        )

    def test_citation_doi_details(self):
        details = [d for _, _, d in self.findings("citation-doi")]
        self.assertIn("malformed DOI '10.12/abc': expected 10.<registrant>/<suffix>", details)
        self.assertIn("the link shows doi:10.1016/j.entcs.2005.05.016 but targets 10.1016/j.entcs.2005.05.017",
                      details)

    def test_citation_key_detail(self):
        self.assertEqual(
            [(3, "citation-key", 'citation key ref-xy99 has no reference entry (id="ref-xy99") in doc.md.txt')],
            self.findings("citation-key"))

    def test_ste_counts(self):
        self.assertIn("descriptive sentence has 26 words (limit 25)", self.findings("ste-sentence-length")[0][2])
        self.assertIn("procedural sentence has 21 words (limit 20)", self.findings("ste-procedure-length")[0][2])
        self.assertIn("paragraph has 7 sentences (limit 6)", self.findings("ste-paragraph-length")[0][2])
        self.assertIn("contraction 'doesn't'", self.findings("ste-contraction")[0][2])

    def test_placement_details(self):
        details = sorted(d for _, _, d in self.findings("placement"))
        self.assertEqual(
            sorted([
                "'Bad_Name.md' is not a kebab-case Markdown file name",
                "Bad_Name.md is not linked from the README.md index",
                "diagram source '06A-upper-letter.puml' is not NN[a]-kebab-case.puml",
                "diagram '06A-upper-letter.svg' is not NN[a]-kebab-case.svg",
                "diagram source 'Sample.puml' is not NN[a]-kebab-case.puml",
                "diagram 'Sample.svg' is not NN[a]-kebab-case.svg",
                "unexpected file 'notes.txt' in diagrams/",
            ]),
            details,
        )

    def test_diagram_palette_details(self):
        details = sorted(d for _, _, d in self.findings("diagram-palette"))
        self.assertEqual(2, len(details))
        self.assertIn("02-uncolored.puml includes the palette but uses none of its colors", details)
        self.assertTrue(details[0].startswith("01-sample.puml does not include the shared palette"))

    def test_snippet_corpus_missing_corpus_file_fails(self):
        findings = self.findings("snippet-corpus", "fail-missing-corpus")
        self.assertEqual(1, len(findings), findings)
        self.assertEqual("snippet-corpus", findings[0][1])
        self.assertIn("examples/reified_rspaces/MANIFEST.toml does not exist", findings[0][2])

    def test_mermaid_names_the_plantuml_equivalent(self):
        self.assertIn("PlantUML expresses this type as an activity or component diagram",
                      self.findings("mermaid")[0][2])


class ManifestFileRows(unittest.TestCase):
    """A manifest row may name an example file. The .rho file lives in scratch,
    because the workspace source-normalization gate collects every .rho file."""

    def test_manifest_file_row_matches_and_a_missing_file_is_reported(self):
        with tempfile.TemporaryDirectory(dir=scratch_root()) as tmp:
            case = Path(tmp)
            (case / "test-vectors").mkdir()
            (case / "test-vectors" / "reified-syntax-corpus-v1.json").write_text('{"entries": []}', encoding="utf-8")
            examples = case / "examples" / "reified_rspaces"
            examples.mkdir(parents=True)
            (examples / "send.rho").write_text("new w in {\n  w!(4)\n}\n", encoding="utf-8")
            (examples / "MANIFEST.toml").write_text('[[example]]\nfile = "send.rho"\n', encoding="utf-8")
            doc = case / "doc.md.txt"
            doc.write_text("# Snippet\n\nThe block is an example file.\n\n```rholang\nnew w in { w!(4) }\n```\n",
                           encoding="utf-8")
            _, violations, _, _ = lint_case(case)
            self.assertEqual([], rendered(violations))
            (examples / "MANIFEST.toml").write_text(
                '[[example]]\nfile = "send.rho"\n\n[[example]]\nfile = "gone.rho"\n', encoding="utf-8")
            _, violations, _, _ = lint_case(case)
            self.assertEqual({"snippet-corpus"}, {v.rule for v in violations})
            self.assertIn("lists gone.rho, which does not exist", violations[0].detail)


# ---------------------------------------------------------------------------
# diagram-render: PlantUML through render.sh
# ---------------------------------------------------------------------------


class DiagramRender(unittest.TestCase):
    def copy_case(self, name: str) -> Path:
        tmp = tempfile.TemporaryDirectory(dir=scratch_root())
        self.addCleanup(tmp.cleanup)
        # render.sh keeps the scratch of a failed check for inspection. Point
        # its scratch root into this temporary directory, so the cleanup above
        # also removes the scratch that the negative controls keep.
        environment = mock.patch.dict(os.environ, {"RRS_SCRATCH": str(Path(tmp.name) / "scratch")})
        environment.start()
        self.addCleanup(environment.stop)
        destination = Path(tmp.name) / "case"
        shutil.copytree(FIXTURES / RENDER_RULE / name, destination)
        return destination

    def render(self, case: Path) -> subprocess.CompletedProcess:
        return subprocess.run(["bash", str(RENDER_SH), "--dir", str(case / "diagrams")],
                              capture_output=True, text=True, check=False)

    def write_stub_svg(self, case: Path) -> None:
        (case / "diagrams" / "01-sample.svg").write_text(
            '<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>\n', encoding="utf-8")

    def test_pass_fixture_with_a_fresh_svg(self):
        case = self.copy_case("pass")
        proc = self.render(case)
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        code, violations, _, _ = lint_case(case, render=True)
        self.assertEqual([], rendered(violations))
        self.assertEqual(0, code)
        # A passing render and a passing check leave no scratch behind.
        scratch = Path(os.environ["RRS_SCRATCH"]) / "diagram-render"
        self.assertEqual([], sorted(p.name for p in scratch.iterdir()))

    def test_usage_error_creates_no_scratch(self):
        case = self.copy_case("pass")
        proc = subprocess.run(["bash", str(RENDER_SH), "--dir", str(case / "diagrams"), "palette.txt"],
                              capture_output=True, text=True, check=False)
        self.assertEqual(2, proc.returncode, proc.stdout + proc.stderr)
        self.assertIn("not a .puml source", proc.stderr)
        self.assertFalse((Path(os.environ["RRS_SCRATCH"]) / "diagram-render").exists())

    def test_fail_stale_svg(self):
        case = self.copy_case("pass")
        proc = self.render(case)
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        svg = case / "diagrams" / "01-sample.svg"
        svg.write_text(svg.read_text(encoding="utf-8") + "<!-- stale -->\n", encoding="utf-8")
        code, violations, _, _ = lint_case(case, render=True)
        self.assertEqual({RENDER_RULE}, {v.rule for v in violations}, rendered(violations))
        self.assertTrue(any("differs from the fresh render" in v.detail for v in violations), rendered(violations))
        self.assertEqual(1, code)

    def test_fail_plantuml_syntax_error(self):
        case = self.copy_case("fail")
        self.write_stub_svg(case)
        _, violations, _, _ = lint_case(case, render=True)
        self.assertEqual({RENDER_RULE}, {v.rule for v in violations}, rendered(violations))
        self.assertTrue(any("PlantUML exit status" in v.detail for v in violations), rendered(violations))

    def test_fail_latex_error_reported_only_on_stderr(self):
        # JLaTeXMath does not know \lvert: PlantUML prints a stack trace on
        # stderr but exits with status 0. The check must still fail.
        case = self.copy_case("fail-latex")
        self.write_stub_svg(case)
        _, violations, _, _ = lint_case(case, render=True)
        self.assertEqual({RENDER_RULE}, {v.rule for v in violations}, rendered(violations))
        self.assertTrue(any("PlantUML wrote to stderr" in v.detail for v in violations), rendered(violations))

    def test_render_mode_keeps_a_good_svg_when_the_render_fails(self):
        case = self.copy_case("fail")
        self.write_stub_svg(case)
        before = (case / "diagrams" / "01-sample.svg").read_bytes()
        proc = self.render(case)
        self.assertEqual(1, proc.returncode, proc.stdout + proc.stderr)
        self.assertIn("FAIL 01-sample.puml: PlantUML exit status", proc.stdout)
        self.assertEqual(before, (case / "diagrams" / "01-sample.svg").read_bytes())

    def test_check_refuses_a_scratch_root_under_tmp(self):
        env = dict(os.environ, RRS_SCRATCH="/tmp/rrs-doc-lint-refused", RRS_SANDBOX="0")
        proc = subprocess.run(["bash", str(RENDER_SH), "--check"], capture_output=True, text=True,
                              env=env, check=False)
        self.assertEqual(2, proc.returncode, proc.stdout + proc.stderr)
        self.assertIn("refusing the scratch root", proc.stderr)
        self.assertFalse(Path("/tmp/rrs-doc-lint-refused").exists())


# ---------------------------------------------------------------------------
# Command line contract
# ---------------------------------------------------------------------------


class CommandLine(unittest.TestCase):
    def run_cli(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True,
                              cwd=REPO_ROOT, check=False)

    def test_violation_lines_and_json_summary(self):
        case = FIXTURES / "ste-semicolon" / "fail"
        proc = self.run_cli("--repo-root", str(case), "--no-render-check", "--json", str(case / "doc.md.txt"))
        self.assertEqual(1, proc.returncode, proc.stderr)
        lines = proc.stdout.strip().split("\n")
        self.assertEqual("doc.md.txt:3: ste-semicolon: semicolon in prose: split the sentence", lines[0])
        summary = json.loads(lines[-1])
        self.assertFalse(summary["ok"])
        self.assertEqual(1, summary["violation_count"])
        self.assertEqual({"ste-semicolon": 1}, summary["counts_by_rule"])
        self.assertEqual(list(doc_lint.RULES), summary["rules"])
        self.assertEqual(
            [{"path": "doc.md.txt", "line": 3, "rule": "ste-semicolon",
              "detail": "semicolon in prose: split the sentence"}],
            summary["violations"])

    def test_clean_run_exits_zero(self):
        case = FIXTURES / "ste-semicolon" / "pass"
        proc = self.run_cli("--repo-root", str(case), "--no-render-check", "--json", str(case / "doc.md.txt"))
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertTrue(json.loads(proc.stdout.strip().split("\n")[-1])["ok"])

    def test_missing_file_is_a_usage_error(self):
        proc = self.run_cli("--no-render-check", "no/such/file.md")
        self.assertEqual(2, proc.returncode)
        self.assertIn("no such file", proc.stderr)

    def test_unknown_rule_is_a_usage_error(self):
        proc = self.run_cli("--only", "no-such-rule")
        self.assertEqual(2, proc.returncode)
        self.assertIn("unknown rule", proc.stderr)

    def test_list_rules_names_every_rule(self):
        proc = self.run_cli("--list-rules")
        self.assertEqual(0, proc.returncode)
        names = [line.split(":", 1)[0] for line in proc.stdout.strip().split("\n")]
        self.assertEqual(list(doc_lint.RULES), names)

    def test_design_record_is_exempt(self):
        record = REPO_ROOT / "docs" / "rspace" / "reified-rspaces" / "design-record" / "README.md"
        proc = self.run_cli("--no-render-check", str(record))
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertIn("skipped exempt file", proc.stderr)
        self.assertEqual("", proc.stdout)


# ---------------------------------------------------------------------------
# Building blocks
# ---------------------------------------------------------------------------


class BuildingBlocks(unittest.TestCase):
    def test_github_slugs(self):
        self.assertEqual("decision-1--scope-rule", doc_lint.github_slug("Decision 1 — Scope rule"))
        self.assertEqual("space-id-sid", doc_lint.github_slug(doc_lint.plain_heading_text("Space id (`sid`)")))
        self.assertEqual("0xe0-namespace", doc_lint.github_slug(doc_lint.plain_heading_text("`0xE0` namespace")))
        self.assertEqual("head-only-matching", doc_lint.github_slug("Head-only matching"))

    def test_duplicate_headings_get_numbered_slugs(self):
        blocks = doc_lint.parse_markdown("# A\n\n## Notes\n\n## Notes\n")
        self.assertEqual({"a", "notes", "notes-1"}, doc_lint.anchors_of(blocks))

    def test_explicit_ids_are_anchors(self):
        blocks = doc_lint.parse_markdown('### <a id="decision-18"></a>Decision 18: marker tuples\n')
        self.assertEqual({"decision-18", "decision-18-marker-tuples"}, doc_lint.anchors_of(blocks))

    def test_html_block_swallows_the_next_heading(self):
        # CommonMark: an HTML block of type 6 runs until a blank line.
        blocks = doc_lint.parse_markdown("<details>\n### Hidden\n\n### Shown\n")
        self.assertEqual({"shown"}, doc_lint.anchors_of(blocks))

    def test_sentence_split_keeps_abbreviations(self):
        prose = "Use a short form, e.g. a list. Then stop. The value 1.5 stays."
        self.assertEqual(3, len(doc_lint.split_sentences(prose)))

    def test_procedural_classification(self):
        self.assertTrue(doc_lint.is_procedural("Run the check."))
        self.assertTrue(doc_lint.is_procedural("If the check fails, render the diagram again."))
        self.assertFalse(doc_lint.is_procedural("Record methods: put and remove."))
        self.assertFalse(doc_lint.is_procedural("The check runs."))
        self.assertFalse(doc_lint.is_procedural("Check #3 is a join contact."))
        self.assertFalse(doc_lint.is_procedural("Test suites are listed in the dossier."))
        self.assertTrue(doc_lint.is_procedural("If the check is red, render the diagram again."))

    def test_contractions_and_possessives(self):
        found = [m.group(0) for m in doc_lint.CONTRACTION_RE.finditer(
            "It's the user's space. Let's see. They'd go. The agent's reply isn't long.")]
        self.assertEqual(["It's", "Let's", "They'd", "isn't"], found)

    def test_inline_math_and_code_spans(self):
        spans = doc_lint.scan_spans("A $`x`$ and `y` and ``z ` w``.")
        self.assertEqual([("math", "x"), ("code", "y"), ("code", "z ` w")], [(s.kind, s.content) for s in spans])

    def test_doi_trimming_keeps_balanced_parentheses(self):
        self.assertEqual("10.1016/0304-3975(87)90045-4", doc_lint._trim_doi("10.1016/0304-3975(87)90045-4)."))

    def test_rocq_comment_stripping(self):
        source = "(* Lemma a : True. (* nested *) *)\nLemma b : True.\n"
        self.assertEqual({"b": [2]}, {k: v for k, v in doc_lint_index(source).items()})


def doc_lint_index(source: str) -> dict:
    with tempfile.TemporaryDirectory(dir=scratch_root()) as tmp:
        path = Path(tmp) / "T.v"
        path.write_text(source, encoding="utf-8")
        return doc_lint.rocq_declarations(path)


if __name__ == "__main__":
    unittest.main()
