#!/usr/bin/env python3
"""Run the unit tests of the Reified RSpaces checkers.

Usage: run-checker-tests.py MODULE [MODULE ...]

Each MODULE is the name of a file in scripts/verify-reified/tests/ without
the .py suffix, for example test_closure. The runner loads each module from
its file path and runs its tests with unittest. It prints one summary line
for each module and then a total line.

pgmcp runs the runner as "python3 scripts/verify-reified/run-checker-tests.py
MODULE ...", and a pgmcp checker accepts no interpreter option. So the runner
sets sys.dont_write_bytecode itself, and a run writes no __pycache__
directory into the snapshot.

A module passes when it has at least one test and every test passes. A
skipped test, an expected failure and an unexpected success make the module
fail, because they give no evidence.

Exit codes: 0 every module passed, 1 a module failed, is missing or has no
test, 2 a usage error.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import io
import re
import sys
import traceback
import unittest
from pathlib import Path

TESTS_DIR = Path(__file__).resolve().parent / "tests"
MODULE_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
EXIT_PASS = 0
EXIT_FAIL = 1
EXIT_USAGE = 2


def load_module(name: str, path: Path):
    """Load a test module from its file path and register it under its name."""
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path.name}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        sys.modules.pop(name, None)
        raise
    return module


def run_modules(names: list[str], tests_dir: Path = TESTS_DIR, stream=None) -> int:
    """Run the named modules of tests_dir and return the exit code."""
    stream = sys.stdout if stream is None else stream
    sys.dont_write_bytecode = True
    totals = {"tests": 0, "failures": 0, "errors": 0, "skipped": 0, "missing": 0, "empty": 0}
    failed_modules = 0
    for name in names:
        path = tests_dir / f"{name}.py"
        if not path.is_file():
            totals["missing"] += 1
            failed_modules += 1
            print(f"FAIL: {name}: the module file {name}.py does not exist in {tests_dir.name}/", file=stream)
            continue
        try:
            module = load_module(name, path)
        except (Exception, SystemExit):
            totals["errors"] += 1
            failed_modules += 1
            traceback.print_exc(file=stream)
            print(f"FAIL: {name}: the module raised an error when it was loaded", file=stream)
            continue
        suite = unittest.defaultTestLoader.loadTestsFromModule(module)
        if suite.countTestCases() == 0:
            totals["empty"] += 1
            failed_modules += 1
            print(f"FAIL: {name}: the module has no test", file=stream)
            continue
        result = unittest.TextTestRunner(stream=stream, verbosity=2, buffer=True).run(suite)
        counts = {
            "failures": len(result.failures) + len(result.expectedFailures) + len(result.unexpectedSuccesses),
            "errors": len(result.errors),
            "skipped": len(result.skipped),
        }
        totals["tests"] += result.testsRun
        for key, value in counts.items():
            totals[key] += value
        passed = result.testsRun > 0 and not any(counts.values())
        failed_modules += 0 if passed else 1
        print(f"{'PASS' if passed else 'FAIL'}: {name}: {result.testsRun} tests, {counts['failures']} failures, "
              f"{counts['errors']} errors, {counts['skipped']} skipped", file=stream)
    print(f"checker tests: modules={len(names)} failed_modules={failed_modules} tests={totals['tests']} "
          f"failures={totals['failures']} errors={totals['errors']} skipped={totals['skipped']} "
          f"missing={totals['missing']} empty={totals['empty']}", file=stream)
    return EXIT_PASS if failed_modules == 0 else EXIT_FAIL


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="run-checker-tests.py",
        description="Run the named unit-test modules of scripts/verify-reified/tests/.")
    parser.add_argument("modules", nargs="+", metavar="MODULE",
                        help="a module name in scripts/verify-reified/tests/ without .py, for example test_closure")
    arguments = parser.parse_args(argv)
    for name in arguments.modules:
        if not MODULE_NAME.match(name):
            parser.error(f"{name!r} is not a module name: give the file name without .py and without a directory")
    if len(set(arguments.modules)) != len(arguments.modules):
        parser.error("a module name occurs more than once")
    return run_modules(arguments.modules)


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if isinstance(stream, io.TextIOWrapper):
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")
    sys.exit(main())
