"""Unit tests for the Reified RSpaces checkers.

They cover check-complete.py with its production scan, check-footprint.py with
the pin, the allowed new files and the replaced blocks, and the ORD phase of check-invariant-map.py.
They also cover check-consensus-checklist.py, the citation rule of
check-model-map.py, and its maps of phases other than P1 (Q# and U# ids, no
fix rules, const fn, restricted visibility, associated consts and type aliases).
The P1 id lists of both map checkers include H23 and T6 (decision 36). They also cover
merge-queue-parity-capture.py, which assembles the queue parity golden of P5, and
check-test-port-ledger.py with generate-test-port-ledger.py, the ledger of the legacy tests (P9).

Each checker gets passing fixtures and failing fixtures (negative controls).
Fixtures are built in a temporary directory, so the source tree is never written.
Git fixtures use a hermetic git configuration. They read no system or global file,
and they use a synthetic identity and fixed dates.
"""

import contextlib
import hashlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

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
INVARIANT_MAP = load("check_invariant_map", "check-invariant-map.py")
CHECKLIST = load("check_consensus_checklist", "check-consensus-checklist.py")
QUEUE_PARITY_MERGE = load("merge_queue_parity_capture", "merge-queue-parity-capture.py")
PORT_LEDGER = load("check_test_port_ledger", "check-test-port-ledger.py")
LEDGER_GENERATOR = load("generate_test_port_ledger", "generate-test-port-ledger.py")

GIT_FIXTURE_ENVIRONMENT = {
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_AUTHOR_NAME": "Fixture",
    "GIT_AUTHOR_EMAIL": "fixture@example.com",
    "GIT_COMMITTER_NAME": "Fixture",
    "GIT_COMMITTER_EMAIL": "fixture@example.com",
    "GIT_AUTHOR_DATE": "2026-01-01T00:00:00+0000",
    "GIT_COMMITTER_DATE": "2026-01-01T00:00:00+0000",
}


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_json(path, data):
    write(path, json.dumps(data, indent=2) + "\n")


def run_git(root, *arguments):
    """Run git in ROOT with the hermetic fixture configuration and return its output."""
    environment = {name: value for name, value in os.environ.items() if name not in FOOTPRINT.GIT_LOCATION_VARIABLES}
    environment.update(GIT_FIXTURE_ENVIRONMENT)
    result = subprocess.run(
        ["git", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", "-C", str(root), *arguments],
        env=environment,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(f"git {' '.join(arguments)} failed: {result.stderr.decode('utf-8', 'replace')}")
    return result.stdout.decode("utf-8").strip()


def commit_all(root, message="fixture"):
    """Commit every file of the fixture repository ROOT and return the commit id."""
    run_git(root, "add", "--all")
    run_git(root, "commit", "--quiet", "--message", message)
    return run_git(root, "rev-parse", "HEAD")


def init_repository(root):
    run_git(root, "init", "--quiet")


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


PRODUCTION_ROOTS = ["models/build.rs", "models/src", "rholang/src"]
PRODUCTION_FILES = {
    "models/build.rs": "fn main() {}\n",
    "models/src/lib.rs": "pub fn clean() {}\n",
    "rholang/src/lib.rs": "pub fn clean() {}\n",
    "rholang/tests/a.rs": "fn test() {}\n",
}


class ProductionScan(unittest.TestCase):
    """check-complete.py --production-scan: the plan's grep gate over production roots."""

    def run_case(self, files=None, roots=PRODUCTION_ROOTS):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative, text in {**PRODUCTION_FILES, **(files or {})}.items():
                write(root / relative, text)
            policies = {
                "production_scan": {
                    "description": "fixture",
                    "roots": list(roots),
                    "suffixes": [".rs", ".proto"],
                    "patterns": [{"pattern": "write_exclusive", "reason": "item 8 and M-1"}],
                }
            }
            with contextlib.redirect_stdout(io.StringIO()) as output:
                code = COMPLETE.production_scan(root, policies)
            return code, output.getvalue()

    def test_clean_roots_pass(self):
        code, output = self.run_case()
        self.assertEqual(0, code, output)
        self.assertIn("complete production scan: 3 files, 0 findings", output)

    def test_token_in_production_source_fails(self):
        code, output = self.run_case({"rholang/src/a.rs": "fn a() {}\nlet write_exclusive = 1;\n"})
        self.assertEqual(1, code, output)
        self.assertIn("complete: rholang/src/a.rs:2: consensus token 'write_exclusive' (item 8 and M-1)", output)

    def test_token_in_test_file_passes(self):
        code, output = self.run_case({"rholang/tests/a.rs": "let write_exclusive = 1;\n"})
        self.assertEqual(0, code, output)

    def test_token_in_proto_file_fails(self):
        proto = "message P {\n  bool write_exclusive = 8;\n}\n"
        code, output = self.run_case({"models/src/main/protobuf/a.proto": proto})
        self.assertEqual(1, code, output)
        self.assertIn("complete: models/src/main/protobuf/a.proto:2: consensus token 'write_exclusive'", output)

    def test_token_in_markdown_file_passes(self):
        code, output = self.run_case({"rholang/src/notes.md": "write_exclusive\n"})
        self.assertEqual(0, code, output)

    def test_token_in_file_root_fails(self):
        code, output = self.run_case({"models/build.rs": "// write_exclusive\nfn main() {}\n"})
        self.assertEqual(1, code, output)
        self.assertIn("complete: models/build.rs:1: consensus token 'write_exclusive'", output)

    def test_missing_root_fails(self):
        code, output = self.run_case(roots=PRODUCTION_ROOTS + ["node/src"])
        self.assertEqual(1, code, output)
        self.assertIn("complete: node/src: production root is missing", output)


BASE_MOD = "pub mod a;\npub mod b;\n"
BASE_FIELD = "struct S {\n    field: u8,\n}\n"
PROTECTED = "fn protected() {}\n"
REALIGN_BASE = "fn base() {}\n"
BLOCK_BASE = "fn a() {\n    old_one();\n    old_two();\n}\n\nfn b() {}\n"
BLOCK_OLD = ["    old_one();", "    old_two();"]
BLOCK_NEW = ["    new_call();"]
BLOCK_CURRENT = BLOCK_BASE.replace("    old_one();\n    old_two();", "    new_call();")
COUNTED_LINE = "        + Bound"
COUNTED_BASE = "fn a()\nwhere\n    T: One\n{}\n\nfn b()\nwhere\n    T: One\n{}\n"
COUNTED_CURRENT = COUNTED_BASE.replace("    T: One\n", "    T: One\n" + COUNTED_LINE + "\n")


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

    def test_star_proto_glob_covers_nested_proto_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "a/b/c.proto", "message Base {}\n")
            write_json(root / "digests.json", {"files": {"a/b/c.proto": digest("message Base {}\n")}})
            policies = {"digests": "digests.json", "policies": {"campaign": {"unchanged": ["*.proto"]}}}
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(0, FOOTPRINT.check(root, policies, "campaign"))
                write(root / "a/b/c.proto", "message Changed {}\n")
                self.assertEqual(1, FOOTPRINT.check(root, policies, "campaign"))

    def block_case(self, current, blocks, base=None):
        base = BLOCK_BASE if base is None else base
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "src/block.rs", current)
            write_json(root / "digests.json", {"files": {"src/block.rs": digest(base)}})
            policies = {
                "digests": "digests.json",
                "policies": {
                    "phase": {
                        "unchanged": ["src/**"],
                        "edits": [{"path": "src/block.rs", "replaced_blocks": blocks}],
                    }
                },
            }
            with contextlib.redirect_stdout(io.StringIO()) as output:
                code = FOOTPRINT.check(root, policies, "phase")
            return code, output.getvalue()

    def test_replaced_block_passes(self):
        code, output = self.block_case(BLOCK_CURRENT, [[BLOCK_OLD, BLOCK_NEW]])
        self.assertEqual(0, code, output)
        self.assertIn("footprint policy phase: checked 1 files, 0 violations", output)

    def test_block_added_at_a_unique_anchor_passes(self):
        anchor = ["", "fn b() {}"]
        inserted = ["", "fn helper() {", "    work();", "}", "", "fn b() {}"]
        current = BLOCK_BASE.replace("\nfn b() {}", "\nfn helper() {\n    work();\n}\n\nfn b() {}")
        code, output = self.block_case(current, [[anchor, inserted]])
        self.assertEqual(0, code, output)

    def test_missing_replacement_block_fails(self):
        code, output = self.block_case(BLOCK_BASE, [[BLOCK_OLD, BLOCK_NEW]])
        self.assertEqual(1, code, output)
        self.assertIn("replacement block starting '    new_call();' occurs 0 times, expected 1", output)

    def test_repeated_replacement_block_fails(self):
        current = BLOCK_CURRENT + "fn c() {\n    new_call();\n}\n"
        code, output = self.block_case(current, [[BLOCK_OLD, BLOCK_NEW]])
        self.assertEqual(1, code, output)
        self.assertIn("replacement block starting '    new_call();' occurs 2 times, expected 1", output)

    def test_ambiguous_original_block_fails(self):
        base = BLOCK_BASE + "fn c() {\n    old_one();\n    old_two();\n}\n"
        current = base.replace("    old_one();\n    old_two();", "    new_call();", 1)
        code, output = self.block_case(current, [[BLOCK_OLD, BLOCK_NEW]], base=base)
        self.assertEqual(1, code, output)
        self.assertIn(
            "original block starting '    old_one();' occurs 2 times in the restored file, expected 1", output
        )

    def test_change_beyond_the_block_fails(self):
        current = BLOCK_CURRENT.replace("fn b() {}", "fn b() { extra(); }")
        code, output = self.block_case(current, [[BLOCK_OLD, BLOCK_NEW]])
        self.assertEqual(1, code, output)
        self.assertIn("footprint: src/block.rs: the file holds changes beyond the approved edit", output)

    def test_malformed_block_entry_fails(self):
        for blocks in ([[BLOCK_OLD]], [[BLOCK_OLD, []]], [["    old_one();", BLOCK_NEW]], ["text"]):
            code, output = self.block_case(BLOCK_CURRENT, blocks)
            self.assertEqual(1, code, output)
            self.assertIn("is not a pair of non-empty line lists", output)

    def test_blocks_restore_after_lines(self):
        current = BLOCK_CURRENT.replace("fn a() {", "fn a() -> u8 {").replace("fn b() {}", "fn b() {}\nmod added;")
        edit = {
            "path": "src/block.rs",
            "added_lines": ["mod added;"],
            "replaced_lines": [["fn a() {", "fn a() -> u8 {"]],
            "replaced_blocks": [[BLOCK_OLD, BLOCK_NEW]],
        }
        restored, problems = FOOTPRINT.undo_edit(current, edit)
        self.assertEqual([], problems)
        self.assertEqual(BLOCK_BASE, restored)

    def counted_case(self, current, counts, base=COUNTED_BASE):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "src/bounds.rs", current)
            write_json(root / "digests.json", {"files": {"src/bounds.rs": digest(base)}})
            policies = {
                "digests": "digests.json",
                "policies": {
                    "phase": {
                        "unchanged": ["src/**"],
                        "edits": [{"path": "src/bounds.rs", "added_line_counts": counts}],
                    }
                },
            }
            with contextlib.redirect_stdout(io.StringIO()) as output:
                code = FOOTPRINT.check(root, policies, "phase")
            return code, output.getvalue()

    def test_counted_added_lines_pass(self):
        code, output = self.counted_case(COUNTED_CURRENT, [[COUNTED_LINE, 2]])
        self.assertEqual(0, code, output)
        self.assertIn("footprint policy phase: checked 1 files, 0 violations", output)

    def test_counted_added_line_with_another_count_fails(self):
        for count in (1, 3):
            code, output = self.counted_case(COUNTED_CURRENT, [[COUNTED_LINE, count]])
            self.assertEqual(1, code, output)
            self.assertIn(f"added line {COUNTED_LINE!r} occurs 2 times, expected {count}", output)

    def test_counted_added_line_present_at_the_base_fails(self):
        base = COUNTED_BASE + COUNTED_LINE + "\n"
        current = COUNTED_CURRENT + COUNTED_LINE + "\n"
        code, output = self.counted_case(current, [[COUNTED_LINE, 3]], base=base)
        self.assertEqual(1, code, output)
        self.assertIn("footprint: src/bounds.rs: the file holds changes beyond the approved edit", output)

    def test_malformed_counted_line_entry_fails(self):
        malformed = (
            [[COUNTED_LINE]],
            [[COUNTED_LINE, 0]],
            [[COUNTED_LINE, "2"]],
            [[COUNTED_LINE, True]],
            [[2, 2]],
            [COUNTED_LINE],
        )
        for counts in malformed:
            code, output = self.counted_case(COUNTED_CURRENT, counts)
            self.assertEqual(1, code, output)
            self.assertIn("is not a [line, count] pair with a positive count", output)

    def test_counted_lines_restore_with_the_other_kinds(self):
        base = BLOCK_BASE + COUNTED_BASE
        current = (BLOCK_CURRENT + COUNTED_CURRENT).replace("fn a() {", "fn a() -> u8 {") + "mod added;\n"
        edit = {
            "path": "src/block.rs",
            "added_lines": ["mod added;"],
            "added_line_counts": [[COUNTED_LINE, 2]],
            "replaced_lines": [["fn a() {", "fn a() -> u8 {"]],
            "replaced_blocks": [[BLOCK_OLD, BLOCK_NEW]],
        }
        restored, problems = FOOTPRINT.undo_edit(current, edit)
        self.assertEqual([], problems)
        self.assertEqual(base, restored)

    def test_allowed_new_file_is_exempt_from_no_new_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "src/base.rs", REALIGN_BASE)
            write(root / "src/new.rs", "fn new() {}\n")
            write_json(root / "digests.json", {"files": {"src/base.rs": digest(REALIGN_BASE)}})
            policies = {
                "digests": "digests.json",
                "policies": {
                    "base": {"unchanged": ["src/**"], "no_new_files": ["src/**"]},
                    "phase": {"extends": "base", "allowed_new_files": ["digests.json", "src/new.rs"]},
                },
            }
            with contextlib.redirect_stdout(io.StringIO()) as output:
                code = FOOTPRINT.check(root, policies, "phase")
            self.assertEqual(0, code, output.getvalue())
            write(root / "src/stray.rs", "fn stray() {}\n")
            with contextlib.redirect_stdout(io.StringIO()) as output:
                code = FOOTPRINT.check(root, policies, "phase")
        self.assertEqual(1, code, output.getvalue())
        self.assertIn("footprint: src/stray.rs: new file in a protected directory", output.getvalue())
        self.assertIn("footprint: src/stray.rs: new file outside the allowed list", output.getvalue())
        self.assertNotIn("src/new.rs", output.getvalue())

    def realign_case(self, extra_files=(), allowed=("digests.json", "src/new.rs"), name="realign"):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "src/base.rs", REALIGN_BASE)
            write(root / "src/new.rs", "fn new() {}\n")
            for relative in extra_files:
                write(root / relative, "fn stray() {}\n")
            write_json(root / "digests.json", {"files": {"src/base.rs": digest(REALIGN_BASE)}})
            policies = {
                "digests": "digests.json",
                "policies": {
                    "base": {"unchanged": ["src/**"]},
                    "realign": {"extends": "base", "unchanged": ["**"], "allowed_new_files": list(allowed)},
                    "child": {"extends": "realign"},
                },
            }
            with contextlib.redirect_stdout(io.StringIO()) as output:
                code = FOOTPRINT.check(root, policies, name)
            return code, output.getvalue()

    def test_allowed_new_files_pass(self):
        code, output = self.realign_case()
        self.assertEqual(0, code, output)
        self.assertIn("footprint policy realign: checked 3 files, 0 violations", output)

    def test_unlisted_new_file_fails(self):
        code, output = self.realign_case(extra_files=("src/stray.rs",))
        self.assertEqual(1, code, output)
        self.assertIn("footprint: src/stray.rs: new file outside the allowed list", output)

    def test_allowed_new_files_are_inherited(self):
        code, output = self.realign_case(extra_files=("src/stray.rs",), name="child")
        self.assertEqual(1, code, output)
        self.assertIn("footprint: src/stray.rs: new file outside the allowed list", output)

    def test_missing_allowed_file_fails(self):
        code, output = self.realign_case(allowed=("digests.json", "src/new.rs", "src/absent.rs"))
        self.assertEqual(1, code, output)
        self.assertIn("footprint: src/absent.rs: allowed new file is missing", output)

    def test_git_listing_and_tree_walk_agree(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory) / "repository"
            snapshot = Path(directory) / "snapshot"
            committed = {
                ".gitignore": "ignored.log\nbuild/\n",
                "exec.sh": "#!/bin/sh\n",
                "src/a.rs": "fn a() {}\n",
                "src/deleted.rs": "fn deleted() {}\n",
            }
            for relative, text in committed.items():
                write(repository / relative, text)
            (repository / "exec.sh").chmod(0o755)
            os.symlink("src/a.rs", repository / "link.rs")
            init_repository(repository)
            commit_all(repository)
            (repository / "src/deleted.rs").unlink()
            write(repository / "notes/untracked.txt", "untracked\n")
            write(repository / "ignored.log", "ignored\n")
            write(repository / "build/out.bin", "ignored\n")
            write(repository / "deep/.git/config", "not a repository\n")
            write(repository / "src/.git", "gitdir: elsewhere\n")
            with mock.patch.dict(os.environ, GIT_FIXTURE_ENVIRONMENT):
                from_git = FOOTPRINT.list_tree_files(repository)

            def snapshot_ignore(current, names):
                if Path(current) == repository:
                    return [name for name in names if name in (".git", "ignored.log", "build")]
                return []

            shutil.copytree(repository, snapshot, symlinks=True, ignore=snapshot_ignore)
            from_walk = FOOTPRINT.list_tree_files(snapshot)
            self.assertTrue((snapshot / "deep/.git/config").is_file())
            self.assertTrue((snapshot / "link.rs").is_symlink())
        self.assertEqual([".gitignore", "exec.sh", "notes/untracked.txt", "src/a.rs"], from_git)
        self.assertEqual(from_git, from_walk)

    def test_pin_writes_the_base_digests(self):
        committed = {"a.txt": "alpha\n", "dir/b.rs": "fn b() {}\n", "exec.sh": "#!/bin/sh\n"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative, text in committed.items():
                write(root / relative, text)
            (root / "exec.sh").chmod(0o755)
            init_repository(root)
            base = commit_all(root)
            write(root / "a.txt", "changed after the base commit\n")
            write(root / "uncommitted.txt", "new\n")
            policies = {"digests": "out/digests.json", "base_revision": base, "pinned_roots": ["."]}
            with mock.patch.dict(os.environ, GIT_FIXTURE_ENVIRONMENT), contextlib.redirect_stdout(io.StringIO()):
                FOOTPRINT.pin(root, policies)
                full = (root / "out/digests.json").read_text(encoding="utf-8")
                policies["pinned_roots"] = ["dir", "exec.sh"]
                FOOTPRINT.pin(root, policies)
                partial = json.loads((root / "out/digests.json").read_text(encoding="utf-8"))
        expected = {
            "base_revision": base,
            "roots": ["."],
            "files": {relative: digest(text) for relative, text in committed.items()},
        }
        self.assertEqual(json.dumps(expected, indent=2, sort_keys=True) + "\n", full)
        self.assertEqual({"dir/b.rs": digest("fn b() {}\n"), "exec.sh": digest("#!/bin/sh\n")}, partial["files"])

    def test_pin_fails_closed_on_a_symbolic_link(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "a.txt", "alpha\n")
            os.symlink("a.txt", root / "link.txt")
            init_repository(root)
            base = commit_all(root)
            policies = {"digests": "digests.json", "base_revision": base, "pinned_roots": ["."]}
            with mock.patch.dict(os.environ, GIT_FIXTURE_ENVIRONMENT), self.assertRaises(SystemExit) as raised:
                FOOTPRINT.pin(root, policies)
            self.assertFalse((root / "digests.json").exists())
        self.assertIn(f"link.txt at {base} is a blob with mode 120000, not a regular file", str(raised.exception))

    def test_pin_fails_closed_on_a_submodule(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "a.txt", "alpha\n")
            init_repository(root)
            first = commit_all(root)
            run_git(root, "update-index", "--add", "--cacheinfo", f"160000,{first},sub")
            run_git(root, "commit", "--quiet", "--message", "gitlink")
            base = run_git(root, "rev-parse", "HEAD")
            policies = {"digests": "digests.json", "base_revision": base, "pinned_roots": ["."]}
            with mock.patch.dict(os.environ, GIT_FIXTURE_ENVIRONMENT), self.assertRaises(SystemExit) as raised:
                FOOTPRINT.pin(root, policies)
            self.assertFalse((root / "digests.json").exists())
        self.assertIn(f"sub at {base} is a commit with mode 160000, not a regular file", str(raised.exception))


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


CHECKLIST_TEST_FILE = "rholang/tests/reified_fixture.rs"
CHECKLIST_NESTED_FILE = "casper/tests/util/rholang/fixture_spec.rs"
CHECKLIST_TEST_SOURCE = """use proptest::prelude::*;

#[test]
fn passes() {}

#[tokio::test(
    flavor = "multi_thread",
    worker_threads = 2
)]
async fn async_passes() {}

fn helper() {}

/* An outer comment /* with a nested comment */ still hides this function:
#[test]
fn hidden_in_comment() {}
*/

#[test]
#[ignore]
fn ignored() {}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn prop_in_macro(value in 0u8..4) {
        prop_assert!(value < 4);
    }
}
"""
CHECKLIST_NESTED_SOURCE = """#[tokio::test]
async fn nested_passes() {}
"""


def checklist_test(test, **extra):
    return {"kind": "rust_test", "file": CHECKLIST_TEST_FILE, "test": test, "step": "fixture-tests",
            "proves": "fixture", **extra}


def checklist_map():
    return {
        "schema": "reified-rspaces-consensus-checklist/1",
        "title": "fixture",
        "expected_items": 2,
        "milestones": ["p1", "p2"],
        "footprint_policies": "policies/footprint.json",
        "complete_policies": "policies/complete.json",
        "steps": {
            "fixture-tests": {"recipe": "regression", "family": "RF1", "package": "rholang",
                              "target": "reified_fixture"},
            "casper-fixture": {"recipe": "regression", "family": "RF1", "package": "casper", "target": "mod",
                               "filter": "util::rholang::fixture_spec::"},
            "footprint": {"recipe": "checks", "family": "RF11", "script": "checks/footprint.py",
                          "args": ["--policy", "@milestone"]},
            "complete": {"recipe": "checks", "family": "RF11", "script": "checks/complete.py",
                         "args": ["--phase", "@milestone"]},
            "production-scan": {"recipe": "checks", "family": "RF11", "script": "checks/complete.py",
                                "args": ["--production-scan"]},
        },
        "items": [
            {"number": 1, "id": "first-item", "text": "The first fixture item.", "checks": [
                checklist_test("passes"),
                checklist_test("async_passes"),
                checklist_test("prop_in_macro"),
                {"kind": "rust_test", "file": CHECKLIST_NESTED_FILE, "module": "util::rholang::fixture_spec",
                 "test": "nested_passes", "step": "casper-fixture", "proves": "fixture"},
            ]},
            {"number": 2, "id": "second-item", "text": "The second fixture item.", "checks": [
                {"kind": "footprint", "policy": "@milestone", "mode": "byte_identical",
                 "paths": ["casper/src/protected.rs"], "no_new_files": ["casper/src/**"], "step": "footprint"},
                {"kind": "footprint", "policy": "@milestone", "mode": "approved_edits",
                 "paths": ["casper/src/edited.rs"], "step": "footprint"},
                {"kind": "complete", "scope": "production_scan", "patterns": ["write_exclusive"],
                 "step": "production-scan"},
                {"kind": "complete", "scope": "@milestone", "patterns": ["write_exclusive"], "step": "complete"},
            ]},
        ],
    }


def build_checklist_fixture(root):
    write(root / CHECKLIST_TEST_FILE, CHECKLIST_TEST_SOURCE)
    write(root / CHECKLIST_NESTED_FILE, CHECKLIST_NESTED_SOURCE)
    write(root / "casper/src/protected.rs", "fn protected() {}\n")
    write(root / "casper/src/edited.rs", "fn edited() {}\n// approved\n")
    write_json(root / "policies/digests.json",
               {"files": {"casper/src/edited.rs": digest("fn edited() {}\n"),
                          "casper/src/protected.rs": digest("fn protected() {}\n")}})
    write_json(root / "policies/footprint.json", {
        "digests": "policies/digests.json",
        "policies": {
            "p1": {"unchanged": ["casper/src/**"], "no_new_files": ["casper/src/**"],
                   "edits": [{"path": "casper/src/edited.rs", "added_lines": ["// approved"]}]},
            "p2": {"extends": "p1"},
        },
    })
    write_json(root / "policies/complete.json", {
        "forbidden_anywhere": [{"pattern": "write_exclusive", "reason": "M-1"}],
        "phases": {"p1": {}, "p2": {}},
        "production_scan": {"roots": ["casper/src"], "suffixes": [".rs"],
                            "patterns": [{"pattern": "write_exclusive", "reason": "M-1"}]},
    })
    write(root / "checks/footprint.py", "")
    write(root / "checks/complete.py", "")


class ConsensusChecklistChecker(unittest.TestCase):
    """check-consensus-checklist.py: the map of the 22 consensus-checklist items."""

    def run_map(self, mutate=None, milestone="p1", source=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build_checklist_fixture(root)
            if source is not None:
                write(root / CHECKLIST_TEST_FILE, source)
            data = checklist_map()
            if mutate is not None:
                mutate(data)
            return CHECKLIST.check_map(root, data, milestone)

    def failures(self, report):
        return [line for line in report.lines if line.startswith("FAIL: ")]

    def assert_only_failure(self, report, phrase):
        failures = self.failures(report)
        self.assertEqual(1, len(failures), "\n".join(report.lines))
        self.assertIn(phrase, failures[0])

    def only_checks(self, *checks):
        def mutate(data):
            data["items"][0]["checks"] = list(checks)
        return mutate

    def test_complete_map_passes(self):
        report = self.run_map()
        self.assertEqual(0, report.failed, "\n".join(report.lines))
        self.assertEqual((2, 0, 0), (report.items, report.scheduled, report.retired))
        self.assertEqual(report.checks, report.passed)
        self.assertIn("PASS: step production-scan runs checks/complete.py --production-scan", report.lines)

    def test_item_without_check_fails(self):
        def mutate(data):
            data["items"][1]["checks"] = []
        self.assert_only_failure(self.run_map(mutate), "FAIL: item 2 has no active check at p1")

    def test_missing_test_function_fails(self):
        report = self.run_map(self.only_checks(checklist_test("absent_function"), checklist_test("hidden_in_comment")))
        failures = self.failures(report)
        self.assertEqual(2, len(failures), "\n".join(report.lines))
        self.assertTrue(all(line.endswith("missing test function") for line in failures), failures)

    def test_function_without_test_attribute_fails(self):
        self.assert_only_failure(self.run_map(self.only_checks(checklist_test("helper"))),
                                 "function without a test attribute")

    def test_ignored_test_fails(self):
        self.assert_only_failure(self.run_map(self.only_checks(checklist_test("ignored"))), "ignored test")

    def test_test_inside_proptest_macro_passes(self):
        report = self.run_map(self.only_checks(checklist_test("prop_in_macro")))
        self.assertEqual(0, report.failed, "\n".join(report.lines))
        self.assertIn(f"PASS: item 1 rust_test prop_in_macro ({CHECKLIST_TEST_FILE}, step fixture-tests)",
                      report.lines)

    def test_multiline_tokio_attribute_passes(self):
        report = self.run_map(self.only_checks(checklist_test("async_passes")))
        self.assertEqual(0, report.failed, "\n".join(report.lines))
        self.assertIn(f"PASS: item 1 rust_test async_passes ({CHECKLIST_TEST_FILE}, step fixture-tests)",
                      report.lines)

    def test_nested_module_test_passes(self):
        nested = {"kind": "rust_test", "file": CHECKLIST_NESTED_FILE, "module": "util::rholang::fixture_spec",
                  "test": "nested_passes", "step": "casper-fixture"}
        report = self.run_map(self.only_checks(nested))
        self.assertEqual(0, report.failed, "\n".join(report.lines))
        self.assertIn(f"PASS: item 1 rust_test nested_passes ({CHECKLIST_NESTED_FILE}, step casper-fixture)",
                      report.lines)

    def test_unknown_policy_fails(self):
        def mutate(data):
            data["items"][1]["checks"][0]["policy"] = "absent"
        self.assert_only_failure(self.run_map(mutate), "unknown footprint policy absent")

    def test_edited_path_is_not_byte_identical(self):
        def mutate(data):
            data["items"][1]["checks"][1]["mode"] = "byte_identical"
        self.assert_only_failure(self.run_map(mutate),
                                 "casper/src/edited.rs: 1 of 1 pinned files are not byte-identical under policy p1")

    def test_unpinned_glob_fails(self):
        def mutate(data):
            data["items"][1]["checks"][0]["paths"] = ["casper/src/absent/**"]
        self.assert_only_failure(self.run_map(mutate), "casper/src/absent/** names no pinned file (re-pin)")

    def test_missing_no_new_files_glob_fails(self):
        def mutate(data):
            data["items"][1]["checks"][0]["no_new_files"] = ["casper/src/**", "rholang/src/**"]
        self.assert_only_failure(self.run_map(mutate), "policy p1 has no no_new_files glob rholang/src/**")

    def test_allowed_new_file_in_a_protected_glob_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build_checklist_fixture(root)
            control = CHECKLIST.check_map(root, checklist_map(), "p2")
            self.assertEqual([], self.failures(control), "\n".join(control.lines))
            policies = json.loads((root / "policies/footprint.json").read_text(encoding="utf-8"))
            policies["policies"]["p2"]["allowed_new_files"] = ["casper/src/new.rs", "docs/new.md"]
            write_json(root / "policies/footprint.json", policies)
            report = CHECKLIST.check_map(root, checklist_map(), "p2")
        self.assert_only_failure(report, "policy p2 allows new files in casper/src/** (casper/src/new.rs)")

    def test_missing_forbidden_pattern_fails(self):
        def mutate(data):
            data["items"][1]["checks"][2]["patterns"] = ["write_exclusive", "suffix_key"]
            data["items"][1]["checks"][3]["patterns"] = ["write_exclusive", "suffix_key"]
        report = self.run_map(mutate)
        failures = self.failures(report)
        self.assertEqual(2, len(failures), "\n".join(report.lines))
        self.assertIn("the production scan does not forbid suffix_key", failures[0])
        self.assertIn("forbidden_anywhere (phase p1) does not forbid suffix_key", failures[1])

    def test_step_target_mismatch_fails(self):
        def mutate(data):
            data["steps"]["fixture-tests"]["target"] = "other_target"
        failures = self.failures(self.run_map(mutate))
        self.assertEqual(3, len(failures), failures)
        self.assertTrue(all("step target mismatch: step fixture-tests runs target other_target" in line
                            for line in failures), failures)

        def mutate_filter(data):
            data["steps"]["casper-fixture"]["filter"] = "util::rholang::other_spec::"
        self.assert_only_failure(self.run_map(mutate_filter),
                                 "the filter util::rholang::other_spec:: of step casper-fixture does not select "
                                 "util::rholang::fixture_spec::nested_passes")

    def test_scheduled_check_skipped_before_from(self):
        def mutate(data):
            data["items"][0]["checks"].append(checklist_test("written_in_p2", **{"from": "p2", "owner": "P2"}))
            data["items"][0]["checks"].append(checklist_test("passes", until="p1"))
        at_p1 = self.run_map(mutate, milestone="p1")
        self.assertEqual((0, 1, 0), (at_p1.failed, at_p1.scheduled, at_p1.retired), "\n".join(at_p1.lines))
        self.assertIn("active from p2 (owner P2)", next(line for line in at_p1.lines if line.startswith("SCHEDULED")))
        at_p2 = self.run_map(mutate, milestone="p2")
        self.assertEqual((1, 0, 1), (at_p2.failed, at_p2.scheduled, at_p2.retired), "\n".join(at_p2.lines))
        self.assert_only_failure(at_p2, "rust_test written_in_p2")

    def test_item_with_only_scheduled_checks_fails(self):
        def mutate(data):
            for check in data["items"][1]["checks"]:
                check["from"] = "p2"
        report = self.run_map(mutate)
        self.assertEqual(4, report.scheduled, "\n".join(report.lines))
        self.assert_only_failure(report, "FAIL: item 2 has no active check at p1")

    def test_wrong_numbering_fails(self):
        def renumber(data):
            data["items"][1]["number"] = 3
        self.assert_only_failure(self.run_map(renumber), "the items are numbered [1, 3], expected 1..2")

        def repeat_id(data):
            data["items"][1]["id"] = "first-item"
        self.assert_only_failure(self.run_map(repeat_id), "the item ids first-item occur more than once")

    def test_unknown_schema_exits_2(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build_checklist_fixture(root)
            data = checklist_map()
            data["schema"] = "reified-rspaces-consensus-checklist/0"
            write_json(root / "map.json", data)
            arguments = ["check-consensus-checklist.py", "--root", str(root), "--map", str(root / "map.json")]
            with mock.patch.object(sys, "argv", arguments), contextlib.redirect_stderr(io.StringIO()) as errors, \
                    contextlib.redirect_stdout(io.StringIO()):
                code = CHECKLIST.main()
        self.assertEqual(2, code)
        self.assertIn("unknown schema 'reified-rspaces-consensus-checklist/0'", errors.getvalue())

    def test_missing_checker_script_fails(self):
        def mutate(data):
            data["steps"]["footprint"]["script"] = "checks/absent.py"
        self.assert_only_failure(self.run_map(mutate), "FAIL: step footprint: unknown checker checks/absent.py")

    def test_unterminated_block_comment_fails(self):
        report = self.run_map(source=CHECKLIST_TEST_SOURCE + "/* a comment without an end\n")
        failures = self.failures(report)
        self.assertEqual(3, len(failures), "\n".join(report.lines))
        self.assertTrue(all(f"{CHECKLIST_TEST_FILE}: a block comment has no end" in line for line in failures))


ORDINARY_TEST_FILE = "rholang/tests/reified_ordinary_regression.rs"
ORDINARY_CHECKLIST_FILE = "scripts/verify-reified/consensus-checklist.json"
ORDINARY_SOURCE = """#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r9_play_replay_equality() {}

proptest! {
    #![proptest_config(property_config())]
    #[test]
    fn prop_ordinary_play_replay_equal(case in ordinary_case()) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_play_replay_negative_control_detects_mutants() {}
"""


def ordinary_phase():
    return {
        "title": "Ordinary execution (fixture)",
        "design": "fixture",
        "test_status": "required",
        "test_files": [ORDINARY_TEST_FILE],
        "expected_properties": 1,
        "formal": {
            "O1": {
                "kind": "checklist",
                "name": "host-replay-acceptance",
                "item": 10,
                "file": ORDINARY_CHECKLIST_FILE,
                "tests": ["prop_ordinary_play_replay_equal"],
            }
        },
        "properties": [
            {"number": 1, "test": "prop_ordinary_play_replay_equal", "file": ORDINARY_TEST_FILE, "covers": ["O1"]}
        ],
        "witness_tests": [
            {"test": "reified_ordinary_regression_r9_play_replay_equality", "file": ORDINARY_TEST_FILE,
             "rule": "R9", "covers": ["O1"]},
            {"test": "ordinary_play_replay_negative_control_detects_mutants", "file": ORDINARY_TEST_FILE,
             "rule": "negative control", "covers": [], "unmodeled": "A negative control."},
        ],
    }


def ordinary_checklist(item_id="host-replay-acceptance", tests=("reified_ordinary_regression_r9_play_replay_equality",
                                                                 "prop_ordinary_play_replay_equal")):
    checks = [{"kind": "rust_test", "file": ORDINARY_TEST_FILE, "test": test, "step": "regression-ordinary"}
              for test in tests]
    checks.append({"kind": "footprint", "policy": "@milestone", "mode": "byte_identical",
                   "paths": ["rspace++/src/rspace/replay_rspace/**"], "step": "footprint"})
    return {
        "schema": "reified-rspaces-consensus-checklist/1",
        "items": [{"number": 10, "id": item_id, "text": "Host replay acceptance is unchanged.", "checks": checks}],
    }


class InvariantMapOrdinaryPhase(unittest.TestCase):
    """check-invariant-map.py: the ORD phase and the checklist kind."""

    def run_case(self, phase=None, checklist=None, source=ORDINARY_SOURCE):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / ORDINARY_TEST_FILE, source)
            write_json(root / ORDINARY_CHECKLIST_FILE, checklist or ordinary_checklist())
            report = INVARIANT_MAP.Report()
            selected = phase or ordinary_phase()
            with mock.patch.object(INVARIANT_MAP, "REPO_ROOT", root), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                INVARIANT_MAP.check_phase("ORD", selected, 1, INVARIANT_MAP.Formal(), report)
                INVARIANT_MAP.check_tests("ORD", selected, report)
            return report, output.getvalue()

    def test_ordinary_phase_passes(self):
        report, output = self.run_case()
        self.assertEqual(0, report.failed, output)
        self.assertIn("PASS: ORD O1 host-replay-acceptance -> prop_ordinary_play_replay_equal", output)
        self.assertIn("PASS: ORD: test function prop_ordinary_play_replay_equal exists", output)

    def test_unknown_checklist_item_fails(self):
        phase = ordinary_phase()
        phase["formal"]["O1"]["item"] = 23
        report, output = self.run_case(phase=phase)
        self.assertEqual(1, report.failed, output)
        self.assertIn("FAIL: ORD O1: unknown checklist item 23", output)

    def test_checklist_id_mismatch_fails(self):
        report, output = self.run_case(checklist=ordinary_checklist(item_id="host-replay"))
        self.assertEqual(1, report.failed, output)
        self.assertIn("FAIL: ORD O1: checklist item 10 has the id 'host-replay', not 'host-replay-acceptance'", output)

    def test_checklist_item_must_name_the_test(self):
        checklist = ordinary_checklist(tests=("reified_ordinary_regression_r9_play_replay_equality",))
        report, output = self.run_case(checklist=checklist)
        self.assertEqual(1, report.failed, output)
        self.assertIn("names no rust_test check for prop_ordinary_play_replay_equal", output)

    def test_missing_property_test_fails(self):
        source = ORDINARY_SOURCE.replace("fn prop_ordinary_play_replay_equal", "fn renamed_property")
        report, output = self.run_case(source=source)
        self.assertEqual(1, report.failed, output)
        self.assertIn("FAIL: ORD: missing test function prop_ordinary_play_replay_equal", output)

    def test_unmapped_prop_function_fails(self):
        report, output = self.run_case(source=ORDINARY_SOURCE + "\nfn prop_generator_helper() {}\n")
        self.assertEqual(1, report.failed, output)
        self.assertIn(f"FAIL: ORD: unmapped property test prop_generator_helper in {ORDINARY_TEST_FILE}", output)

    def test_missing_required_id_fails(self):
        phase = ordinary_phase()
        phase["formal"]["O2"] = phase["formal"].pop("O1")
        report, output = self.run_case(phase=phase)
        self.assertLessEqual(1, report.failed, output)
        self.assertIn("FAIL: ORD: unmapped: O1 is not in the invariant map", output)

    def test_phase_without_tla_entries_keeps_auxiliary_invariants(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / ORDINARY_TEST_FILE, ORDINARY_SOURCE)
            write_json(root / ORDINARY_CHECKLIST_FILE, ordinary_checklist())
            write(root / "tla/M.tla", "---- MODULE M ----\nVARIABLE x\nTypeOK == x \\in {0, 1}\nInv == x = 0\n====\n")
            write(root / "tla/M.cfg", "INVARIANTS TypeOK Inv\n")
            write(root / "tests/model.rs", "#[test]\nfn prop_model_inv() {}\n")
            invariant_map = {
                "schema": INVARIANT_MAP.SCHEMA,
                "tla_config_dir": "tla",
                "auxiliary_tla_invariants": {"TypeOK": "The type invariant of the fixture module."},
                "phases": {
                    "PT": {
                        "test_status": "required",
                        "test_files": ["tests/model.rs"],
                        "expected_properties": 1,
                        "formal": {"T1": {"kind": "tla", "name": "Inv", "file": "tla/M.tla",
                                          "tests": ["prop_model_inv"]}},
                        "properties": [{"number": 1, "test": "prop_model_inv", "file": "tests/model.rs",
                                        "covers": ["T1"]}],
                    },
                    "ORD": ordinary_phase(),
                },
            }
            write_json(root / "map.json", invariant_map)
            arguments = ["check-invariant-map.py", "--map", str(root / "map.json"), "--phase", "ORD"]
            with mock.patch.object(INVARIANT_MAP, "REPO_ROOT", root), mock.patch.object(sys, "argv", arguments), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                code = INVARIANT_MAP.main()
        self.assertEqual(0, code, output.getvalue())
        self.assertIn("FAIL=0", output.getvalue())


PHASE_THEORY = """Definition head := 0.
Theorem queue_head_is_min_ord : True. Proof. exact I. Qed.
Print Assumptions queue_head_is_min_ord.
Theorem route_rules : True. Proof. exact I. Qed.
Print Assumptions route_rules.
"""

PHASE_MODULE = """---- MODULE Model ----
Produce == TRUE
HeadBlocks == TRUE
====
"""

PHASE_SOURCE = """pub const fn is_call_op(op: u8) -> bool { op <= 3 }

pub type SpaceCtx = Option<u8>;

pub struct Core;

impl Core {
    pub const LIMIT: usize = 64;

    pub(in crate::rust::interpreter) fn reified_consume<'a>(&'a self) {}

    pub(super) async fn eval_marker(&self) {}
}
"""


def phase_item(item, name, kind, covers, scope=None, **extra):
    entry = {"item": item, "required": True, "file": "src/space.rs", "kind": kind, "name": name, "covers": covers}
    if scope is not None:
        entry["scope"] = scope
    entry.update(extra)
    return entry


def phase_model_map(**changes):
    data = {
        "schema": "reified-rspaces-model-map/2",
        "phase": "P4",
        "formal_files": {"rocq": ["theories/Model.v"], "tla": ["tla/Model.tla"]},
        "headlines": {
            "Q1": {"kind": "rocq", "name": "queue_head_is_min_ord", "file": "theories/Model.v"},
            "Q2": {"kind": "rocq", "name": "route_rules", "file": "theories/Model.v"},
            "U1": {"kind": "tla", "name": "HeadBlocks", "file": "tla/Model.tla"},
        },
        "items": [
            phase_item("is_call_op", "is_call_op", "fn", ["Q1"], rocq=["head"]),
            phase_item("SpaceCtx", "SpaceCtx", "type", ["Q2"]),
            phase_item("Core::LIMIT", "LIMIT", "const", ["U1"], scope="Core", tla=["Produce"]),
            phase_item("Core::reified_consume", "reified_consume", "fn", ["Q1"], scope="Core"),
            phase_item("Core::eval_marker", "eval_marker", "fn", ["Q2"], scope="Core"),
        ],
    }
    data.update(changes)
    return data


class ModelMapPhase(unittest.TestCase):
    """check-model-map.py: the map of a phase other than P1."""

    def run_map(self, data, formal_only=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "theories/Model.v", PHASE_THEORY)
            write(root / "tla/Model.tla", PHASE_MODULE)
            write(root / "src/space.rs", PHASE_SOURCE)
            report = MODEL_MAP.Report()
            with mock.patch.object(MODEL_MAP, "REPO_ROOT", root), contextlib.redirect_stdout(io.StringIO()) as output:
                MODEL_MAP.check(data, formal_only, report)
            return report, output.getvalue()

    def test_phase_map_with_letter_ids_passes(self):
        report, output = self.run_map(phase_model_map())
        self.assertEqual(0, report.failed, output)
        self.assertIn("PASS: Q1 = theorem queue_head_is_min_ord", output)
        self.assertIn("PASS: U1 = invariant HeadBlocks", output)

    def test_phase_map_needs_no_fix_rules(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "map.json"
            write_json(path, phase_model_map())
            self.assertEqual("P4", MODEL_MAP.load_map(path)["phase"])

    def test_p1_map_still_needs_fix_rules(self):
        data = phase_model_map()
        del data["phase"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "map.json"
            write_json(path, data)
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                MODEL_MAP.load_map(path)
        self.assertEqual(2, raised.exception.code)

    def test_item_that_cites_an_unknown_headline_fails(self):
        data = phase_model_map()
        data["items"][0]["covers"] = ["Q1", "Q9"]
        report, output = self.run_map(data, formal_only=True)
        self.assertEqual(1, report.failed, output)
        self.assertIn("cites unknown headline Q9", output)

    def test_headline_without_an_item_fails(self):
        data = phase_model_map()
        data["items"][2]["covers"] = ["Q1"]
        report, output = self.run_map(data, formal_only=True)
        self.assertEqual(1, report.failed, output)
        self.assertIn("U1 (HeadBlocks) covers no Rust item", output)

    def test_headline_id_with_two_letters_fails(self):
        data = phase_model_map()
        data["headlines"]["QA1"] = data["headlines"].pop("Q1")
        for entry in data["items"]:
            entry["covers"] = ["QA1" if hid == "Q1" else hid for hid in entry["covers"]]
        report, output = self.run_map(data, formal_only=True)
        self.assertEqual(1, report.failed, output)
        self.assertIn("headline id QA1 is not one letter and a number", output)

    def test_item_absent_from_the_code_fails(self):
        data = phase_model_map()
        data["items"].append(phase_item("Core::absent", "absent", "fn", ["Q1"], scope="Core"))
        report, output = self.run_map(data)
        self.assertEqual(1, report.failed, output)
        self.assertIn("code: Core::absent: fn absent is not in impl Core of src/space.rs", output)

    def test_item_lookup_reads_const_fn_restricted_visibility_consts_and_types(self):
        lines = PHASE_SOURCE.split("\n")
        self.assertEqual(1, MODEL_MAP.find_rust_item(lines, "fn", "is_call_op", None))
        self.assertEqual(3, MODEL_MAP.find_rust_item(lines, "type", "SpaceCtx", None))
        self.assertEqual(8, MODEL_MAP.find_rust_item(lines, "const", "LIMIT", "Core"))
        self.assertIsNone(MODEL_MAP.find_rust_item(lines, "const", "LIMIT", None))
        self.assertEqual(10, MODEL_MAP.find_rust_item(lines, "fn", "reified_consume", "Core"))
        self.assertEqual(12, MODEL_MAP.find_rust_item(lines, "fn", "eval_marker", "Core"))
        self.assertIsNone(MODEL_MAP.find_rust_item(lines, "fn", "eval_marker", None))

    def test_ids_sort_by_letters_then_number(self):
        self.assertEqual(["Q2", "Q10", "U1"], sorted(["U1", "Q10", "Q2"], key=MODEL_MAP.id_key))


P1_THEORY = "".join(f"Theorem h{i} : True. Proof. exact I. Qed.\nPrint Assumptions h{i}.\n" for i in range(1, 25))
P1_MODULE = "---- MODULE P1 ----\n" + "".join(f"T{i}Inv == TRUE\n" for i in range(1, 8)) + "====\n"


def p1_phase(ids):
    formal = {}
    for fid in ids:
        number = int(fid[1:])
        if fid.startswith("H"):
            formal[fid] = {"kind": "rocq", "name": f"h{number}", "file": "theories/P1.v", "tests": ["prop_p1"]}
        else:
            formal[fid] = {"kind": "tla", "name": f"T{number}Inv", "file": "tla/P1.tla", "tests": ["prop_p1"]}
    return {
        "test_status": "required",
        "test_files": ["tests/p1.rs"],
        "expected_properties": 1,
        "formal": formal,
        "properties": [{"number": 1, "test": "prop_p1", "file": "tests/p1.rs", "covers": list(ids)}],
    }


class PhaseOneDecision36Ids(unittest.TestCase):
    """check-invariant-map.py and check-model-map.py: the P1 ids include H23 and T6 (decision 36)."""

    def run_phase(self, ids):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "theories/P1.v", P1_THEORY)
            write(root / "tla/P1.tla", P1_MODULE)
            report = INVARIANT_MAP.Report()
            with mock.patch.object(INVARIANT_MAP, "REPO_ROOT", root), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                INVARIANT_MAP.check_phase("P1", p1_phase(ids), 1, INVARIANT_MAP.Formal(), report)
            return report, output.getvalue()

    def test_invariant_map_requires_h23_and_t6(self):
        ids = INVARIANT_MAP.REQUIRED_IDS["P1"]
        self.assertEqual(29, len(ids))
        self.assertIn("H23", ids)
        self.assertIn("T6", ids)
        self.assertNotIn("H24", ids)
        self.assertNotIn("T7", ids)

    def test_complete_p1_phase_passes(self):
        report, output = self.run_phase(INVARIANT_MAP.REQUIRED_IDS["P1"])
        self.assertEqual(0, report.failed, output)
        self.assertIn("PASS: P1 H23 h23 -> prop_p1", output)
        self.assertIn("PASS: P1 T6 T6Inv -> prop_p1", output)

    def test_p1_phase_without_h23_and_t6_fails(self):
        ids = [fid for fid in INVARIANT_MAP.REQUIRED_IDS["P1"] if fid not in ("H23", "T6")]
        report, output = self.run_phase(ids)
        self.assertEqual(2, report.failed, output)
        self.assertIn("FAIL: P1: unmapped: H23 is not in the invariant map", output)
        self.assertIn("FAIL: P1: unmapped: T6 is not in the invariant map", output)

    def test_p1_phase_with_an_id_past_t6_fails(self):
        report, output = self.run_phase(list(INVARIANT_MAP.REQUIRED_IDS["P1"]) + ["T7"])
        self.assertEqual(1, report.failed, output)
        self.assertIn("FAIL: P1: T7 is not an id of phase P1", output)

    def test_model_map_requires_h23_and_t6(self):
        self.assertEqual(29, len(MODEL_MAP.HEADLINE_IDS))
        self.assertIn("H23", MODEL_MAP.HEADLINE_IDS)
        self.assertIn("T6", MODEL_MAP.HEADLINE_IDS)
        data = {
            "schema": "reified-rspaces-model-map/2",
            "formal_files": {"rocq": ["theories/P1.v"], "tla": ["tla/P1.tla"]},
            "headlines": {fid: ({"kind": "rocq", "name": f"h{fid[1:]}", "file": "theories/P1.v"}
                                if fid.startswith("H") else
                                {"kind": "tla", "name": f"{fid}Inv", "file": "tla/P1.tla"})
                          for fid in MODEL_MAP.HEADLINE_IDS if fid not in ("H23", "T6")},
            "rules": {},
            "items": [],
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root / "theories/P1.v", P1_THEORY)
            write(root / "tla/P1.tla", P1_MODULE)
            report = MODEL_MAP.Report()
            with mock.patch.object(MODEL_MAP, "REPO_ROOT", root), contextlib.redirect_stdout(io.StringIO()) as output:
                MODEL_MAP.check(data, True, report)
        self.assertIn("FAIL: H23 is not defined in the map", output.getvalue())
        self.assertIn("FAIL: T6 is not defined in the map", output.getvalue())


class QueueParityMerge(unittest.TestCase):
    """merge-queue-parity-capture.py: one fragment for each scenario, and the --check mode."""

    REVISION = "0" * 40

    def write_fragments(self, directory, skip=None, wrong=None, extra=None):
        for scenario in QUEUE_PARITY_MERGE.SCENARIOS:
            if scenario == skip:
                continue
            key = "another_scenario" if scenario == wrong else scenario
            document = {key: [{"label": scenario, "cost": 1}]}
            (directory / f"{scenario}.json").write_text(json.dumps(document), encoding="utf-8")
        if extra is not None:
            (directory / f"{extra}.json").write_text(json.dumps({extra: []}), encoding="utf-8")

    def arguments(self, directory):
        return types.SimpleNamespace(
            capture_dir=directory,
            output=directory.parent / "golden.json",
            capture_command="capture",
            revision=self.REVISION,
            rustc_version="rustc 0",
            check=False,
        )

    def test_assembles_every_scenario_with_its_provenance(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            self.write_fragments(directory)
            corpus = json.loads(QUEUE_PARITY_MERGE.build_corpus(self.arguments(directory)))
            self.assertEqual(sorted(QUEUE_PARITY_MERGE.SCENARIOS), sorted(corpus["scenarios"]))
            self.assertEqual("reified_queue_parity_v1", corpus["name"])
            self.assertEqual(self.REVISION, corpus["captured_at_revision"])
            self.assertEqual(
                [{"label": "reentry", "cost": 1}], corpus["scenarios"]["reentry"]
            )

    def test_missing_fragment_fails(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            self.write_fragments(directory, skip="reentry")
            with self.assertRaises(SystemExit) as raised:
                QUEUE_PARITY_MERGE.build_corpus(self.arguments(directory))
            self.assertIn("missing capture fragment", str(raised.exception))

    def test_fragment_of_another_scenario_fails(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            self.write_fragments(directory, wrong="legacy_budget")
            with self.assertRaises(SystemExit) as raised:
                QUEUE_PARITY_MERGE.build_corpus(self.arguments(directory))
            self.assertIn("must contain exactly the scenario legacy_budget", str(raised.exception))

    def test_unknown_fragment_fails(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw)
            self.write_fragments(directory, extra="stray")
            with self.assertRaises(SystemExit) as raised:
                QUEUE_PARITY_MERGE.build_corpus(self.arguments(directory))
            self.assertIn("unknown capture fragments: stray", str(raised.exception))

    def test_check_mode_detects_a_changed_golden(self):
        with tempfile.TemporaryDirectory() as raw:
            directory = Path(raw) / "capture"
            directory.mkdir()
            self.write_fragments(directory)
            output = Path(raw) / "golden.json"
            base = [
                "merge-queue-parity-capture.py",
                "--capture-dir", str(directory),
                "--output", str(output),
                "--capture-command", "capture",
                "--revision", self.REVISION,
                "--rustc-version", "rustc 0",
            ]
            with mock.patch.object(sys, "argv", base):
                self.assertEqual(0, QUEUE_PARITY_MERGE.main())
            with mock.patch.object(sys, "argv", base + ["--check"]):
                self.assertEqual(0, QUEUE_PARITY_MERGE.main())
            output.write_text(output.read_text(encoding="utf-8").replace('"cost": 1', '"cost": 2', 1),
                              encoding="utf-8")
            with mock.patch.object(sys, "argv", base + ["--check"]), \
                    contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertEqual(1, QUEUE_PARITY_MERGE.main())
            self.assertIn("differs from the assembled capture", errors.getvalue())


def ledger_entry(name, module=""):
    return {"path": "legacy/tests.rs", "module": module, "name": name, "kind": "test", "line": 1}


def ledger_row(name, **fields):
    row = {
        "path": "legacy/tests.rs",
        "module": "",
        "name": name,
        "kind": "test",
        "behavior": f"area: {name}",
        "disposition": "port",
        "successors": [{"file": "tests/new.rs", "test": "successor_test"}],
    }
    row.update(fields)
    return row


class TestPortLedgerChecker(unittest.TestCase):
    """check-test-port-ledger.py: one row for each inventory entry, existing successors, the
    reason of a drop, and the non-match counterpart of a positive matcher test."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        write(
            self.root / "tests/new.rs",
            "#[test]\nfn successor_test() {}\n\n#[test]\nasync fn non_match_test() {}\n\n"
            "#[test]\nfn prefix_test_longer() {}\n",
        )
        write_json(self.root / "inventory.json", {"tests": [ledger_entry("first"), ledger_entry("second")]})

    def tearDown(self):
        self.temp.cleanup()

    def check(self, rows):
        write_json(self.root / "ledger.json", {"rows": rows})
        output = io.StringIO()
        with mock.patch.object(PORT_LEDGER, "ROOT", self.root), \
                mock.patch.object(PORT_LEDGER, "INVENTORY", self.root / "inventory.json"), \
                mock.patch.object(PORT_LEDGER, "LEDGER", self.root / "ledger.json"), \
                mock.patch.object(sys, "argv", ["check-test-port-ledger.py"]), \
                contextlib.redirect_stdout(output):
            code = PORT_LEDGER.main()
        return code, output.getvalue()

    def assert_fails(self, rows, message):
        code, output = self.check(rows)
        self.assertEqual(code, 1, output)
        self.assertIn(message, output)

    def test_complete_ledger_passes(self):
        code, output = self.check([
            ledger_row("first"),
            ledger_row("second", disposition="drop", reason="decision 7: the channel layout changed"),
        ])
        self.assertEqual(code, 0, output)
        self.assertIn("test-port ledger: PASS", output)

    def test_missing_row_fails(self):
        self.assert_fails([ledger_row("first")], "legacy/tests.rs::::second: 0 rows, expected 1")

    def test_duplicate_row_fails(self):
        self.assert_fails([ledger_row("first"), ledger_row("first"), ledger_row("second")], "2 rows, expected 1")

    def test_row_without_entry_fails(self):
        self.assert_fails([ledger_row("first"), ledger_row("second"), ledger_row("third")],
                          "row names no inventory entry")

    def test_unknown_disposition_fails(self):
        self.assert_fails([ledger_row("first", disposition="skip"), ledger_row("second")],
                          "disposition 'skip' is not one of")

    def test_missing_behavior_fails(self):
        self.assert_fails([ledger_row("first", behavior=""), ledger_row("second")], "first: no behavior")

    def test_missing_successor_fails(self):
        self.assert_fails([ledger_row("first", successors=[]), ledger_row("second")], "first: no successor")

    def test_successor_must_exist_by_exact_name(self):
        missing = [{"file": "tests/new.rs", "test": "prefix_test"}]
        self.assert_fails([ledger_row("first", successors=missing), ledger_row("second")],
                          "successor tests/new.rs::prefix_test does not exist")

    def test_successor_in_missing_file_fails(self):
        missing = [{"file": "tests/absent.rs", "test": "successor_test"}]
        self.assert_fails([ledger_row("first", successors=missing), ledger_row("second")],
                          "successor tests/absent.rs::successor_test does not exist")

    def test_drop_needs_a_reason_naming_a_decision(self):
        self.assert_fails([ledger_row("first", disposition="drop", reason="obsolete"), ledger_row("second")],
                          "a drop needs a reason that names a decision or a plan section")

    def test_positive_match_needs_a_non_match_counterpart(self):
        self.assert_fails([ledger_row("first", positive_match=True), ledger_row("second")],
                          "needs a non_match counterpart")

    def test_non_match_counterpart_must_exist(self):
        counterpart = [{"file": "tests/new.rs", "test": "absent_test"}]
        self.assert_fails([ledger_row("first", positive_match=True, non_match=counterpart), ledger_row("second")],
                          "non_match tests/new.rs::absent_test does not exist")

    def test_positive_match_with_an_existing_counterpart_passes(self):
        counterpart = [{"file": "tests/new.rs", "test": "non_match_test"}]
        code, output = self.check([ledger_row("first", positive_match=True, non_match=counterpart),
                                   ledger_row("second")])
        self.assertEqual(code, 0, output)

    def test_missing_ledger_fails(self):
        output = io.StringIO()
        with mock.patch.object(PORT_LEDGER, "ROOT", self.root), \
                mock.patch.object(PORT_LEDGER, "INVENTORY", self.root / "inventory.json"), \
                mock.patch.object(PORT_LEDGER, "LEDGER", self.root / "absent.json"), \
                mock.patch.object(sys, "argv", ["check-test-port-ledger.py"]), \
                contextlib.redirect_stdout(output):
            self.assertEqual(PORT_LEDGER.main(), 1)
        self.assertIn("is missing", output.getvalue())


class TestPortLedgerGenerator(unittest.TestCase):
    """generate-test-port-ledger.py: the first matching rule gives a row, every entry needs a
    rule, every rule needs an entry, and a positive matcher rule marks its rows."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        write_json(self.root / "inventory.json", {
            "base": "a",
            "head": "b",
            "tests": [ledger_entry("test_first"), ledger_entry("test_second", module="unit_tests")],
        })

    def tearDown(self):
        self.temp.cleanup()

    def build(self, rules):
        output = io.StringIO()
        with mock.patch.object(LEDGER_GENERATOR, "INVENTORY", self.root / "inventory.json"), \
                mock.patch.object(LEDGER_GENERATOR, "RULES", rules), \
                mock.patch.object(LEDGER_GENERATOR, "FILES", {"new": "tests/new.rs"}), \
                mock.patch.object(LEDGER_GENERATOR, "OVERRIDES", {}), \
                contextlib.redirect_stdout(output):
            try:
                return LEDGER_GENERATOR.build(), output.getvalue()
            except SystemExit:
                return None, output.getvalue()

    def test_first_matching_rule_gives_the_row(self):
        rule = LEDGER_GENERATOR.rule
        ledger, output = self.build([
            rule("legacy/tests.rs", "test_first", "area", "port", ["new:successor_test"], nm=["new:non_match_test"]),
            rule("legacy/tests.rs", None, "area", "drop", ["new:successor_test"], "decision 7: changed"),
            rule("legacy/tests.rs", "test_second", "unused", "port", ["new:successor_test"]),
        ])
        self.assertIsNone(ledger, output)
        self.assertIn("rule matches no entry: legacy/tests.rs test_second", output)

    def test_rows_follow_their_rules(self):
        rule = LEDGER_GENERATOR.rule
        ledger, output = self.build([
            rule("legacy/tests.rs", "test_first", "area", "port", ["new:successor_test"], nm=["new:non_match_test"]),
            rule("legacy/tests.rs", "test_second", "area", "drop", ["new:successor_test"], "decision 7: changed",
                 module="unit_tests"),
        ])
        self.assertIsNotNone(ledger, output)
        first, second = ledger["rows"]
        self.assertEqual(first["behavior"], "area: first")
        self.assertTrue(first["positive_match"])
        self.assertEqual(first["non_match"], [{"file": "tests/new.rs", "test": "non_match_test"}])
        self.assertNotIn("reason", first)
        self.assertEqual(second["disposition"], "drop")
        self.assertEqual(second["reason"], "decision 7: changed")
        self.assertNotIn("positive_match", second)

    def test_entry_without_a_rule_fails(self):
        rule = LEDGER_GENERATOR.rule
        ledger, output = self.build([
            rule("legacy/tests.rs", "test_first", "area", "port", ["new:successor_test"]),
        ])
        self.assertIsNone(ledger, output)
        self.assertIn("no rule matches legacy/tests.rs::unit_tests::test_second", output)

    def test_module_filter_excludes_other_modules(self):
        rule = LEDGER_GENERATOR.rule
        ledger, output = self.build([
            rule("legacy/tests.rs", "test_(first|second)", "area", "port", ["new:successor_test"], module=""),
            rule("legacy/tests.rs", None, "other", "port", ["new:successor_test"]),
        ])
        self.assertIsNotNone(ledger, output)
        self.assertEqual([row["behavior"] for row in ledger["rows"]], ["area: first", "other: second"])


if __name__ == "__main__":
    unittest.main()
