#!/usr/bin/env python3
"""Mutation controls of phase P7 (design v3 section 11).

Each mutant breaks one rule of P7 where a passing test could be vacuous: the volatile sweep and
its root predicate (VR-3, VR-4, VR-8), the dangling checks of temp spaces (DG-1), the Seq rules
(SQ-2 to SQ-4), the theory seam (TH-5, TH-6), the HashSet store (HS-2) and the qualifier gate
of legacy execution (QT-5). The two theory mutants are the dry runs of P7 step 4.

The harness applies one mutant at a time, runs the tests that the mutant names, and restores
the source files. A mutant passes the control when at least one of its tests fails. A mutant
may change several places, when its rule has several places in the code.

Rules of the harness:
  * The harness reads the source files first and restores them after every mutant, also on an
    error or an interrupt. It prints whether the restored files equal the originals.
  * Each mutation text must occur exactly once in its file. Otherwise the control fails as
    stale.
  * Unless RRS_SANDBOX=1, each cargo run is bounded by a systemd user scope (MemoryMax,
    CPUQuota). TMPDIR points into target/, never into the RAM-backed temporary directory.

Usage: python3 scripts/verify-reified/p7-mutants.py [--log-dir DIR] [--only NAME ...]
Exit codes: 0 every mutant fails at least one of its tests, 1 a mutant survived or is stale.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INTERPRETER = REPO_ROOT / "rholang/src/rust/interpreter"
REDUCE = INTERPRETER / "reduce.rs"
HOOKS = INTERPRETER / "reified/reducer_hooks.rs"
RUNTIME = INTERPRETER / "reified/runtime.rs"
SEQ = INTERPRETER / "reified/seq.rs"
EPHEMERAL = INTERPRETER / "spaces/ephemeral.rs"
GENERIC = INTERPRETER / "spaces/generic.rs"
HASHSET = INTERPRETER / "spaces/outer/hashset.rs"
CONSTRUCTOR = INTERPRETER / "spaces/factory/constructor.rs"

# A test run: the cargo target arguments and the test filters. "--lib" runs the library tests.
LIB = ("--lib",)


def suite(name: str) -> tuple[str, ...]:
    return ("--test", name)


# (name, rule, edits, runs). Each edit is (file, old text, new text). Each run is (target
# arguments, filters). The mutant is killed when a test of a run fails.
MUTANTS = (
    ("sweep-skipped-at-one-entry",
     "VR-4: inj_with_host_work does not sweep",
     ((REDUCE,
       "            .eval_with_host_work(par, &Env::new(), rand, host_work)\n"
       "            .await;\n"
       "        self.end_root_evaluation(root, result).await\n",
       "            .eval_with_host_work(par, &Env::new(), rand, host_work)\n"
       "            .await;\n"
       "        let _ = root;\n"
       "        result\n"),),
     ((LIB, ("reified_sweep_runs_at_each_entry_point_of_the_reducer",)),)),
    # Changed by P7 step 6 (gate finding 3): cargo fmt split the `match` line, so the anchor of
    # step 6 went stale. It was:
    #   "        match self.reified.sweep_volatile(&self.reduction_coordinator).await {"
    ("sweep-skipped-on-err",
     "VR-8: a root evaluation that fails does not sweep",
     ((HOOKS,
       "        if !root {\n"
       "            return (result, summary);\n"
       "        }\n"
       "        match self\n"
       "            .reified\n"
       "            .sweep_volatile(&self.reduction_coordinator)\n",
       "        if !root || result.is_err() {\n"
       "            return (result, summary);\n"
       "        }\n"
       "        match self\n"
       "            .reified\n"
       "            .sweep_volatile(&self.reduction_coordinator)\n"),),
     ((LIB, ("reified_sweep_runs_at_each_entry_point",)),
      (suite("reified_temp_seq"), ("space_temp_failed_deploy_reverts",)))),
    ("reset-not-guarded",
     "VR-3: a nested call also resets the volatile registry",
     ((HOOKS,
       "        let root = deterministic_reduction::current().is_none();\n"
       "        if root {\n"
       "            self.reified.begin_evaluation_scope();\n"
       "        }\n",
       "        let root = deterministic_reduction::current().is_none();\n"
       "        if root || !root {\n"
       "            self.reified.begin_evaluation_scope();\n"
       "        }\n"),),
     ((LIB, ("reified_sweep_root_predicate_excludes_nested_calls",)),)),
    # Changed by P7 step 6 (gate finding 3): cargo fmt split the arm of an ephemeral space, so
    # the anchor of step 6 went stale. It was:
    #   "            namespace::Durability::Ephemeral => Err(SpaceError::DanglingHandle { space: space.sid }),"
    ("dangling-check-removed",
     "DG-1: the registry check of an ephemeral id passes every id",
     ((RUNTIME,
       "            namespace::Durability::Ephemeral => {\n"
       "                Err(SpaceError::DanglingHandle { space: space.sid })\n"
       "            }\n",
       "            namespace::Durability::Ephemeral => Ok(()),\n"),
      (RUNTIME,
       "            Some(space) if !self.volatile.is_live(&space.sid) => {",
       "            Some(space) if false && !self.volatile.is_live(&space.sid) => {")),
     ((suite("reified_temp_seq"), ("reified_ephemeral_dangling_next_deploy",)),
      (suite("reified_rehydrate"), ("reducer_dangling_handle",)))),
    ("forwarder-hop-scanned",
     "SQ-2 (A1): the forwarder hop to HANDLE_OP is scanned for seq names",
     ((SEQ,
       "    if op_of(chan) == Some(HANDLE_OP) && pars.first().is_some_and(is_bare_handle) {",
       "    if false && op_of(chan) == Some(HANDLE_OP) && pars.first().is_some_and(is_bare_handle) {"),),
     ((LIB, ("reified_seq_scan_exempts_the_forwarder_hop",)),
      (suite("reified_seq"), ("reducer_seq_payload",)))),
    ("reissue-counted-as-use",
     "SQ-3 (A3): an internal job (a re-issue or a settle job) claims channels",
     ((EPHEMERAL,
       "        if qualifier != SpaceQualifier::Seq || internal {",
       "        if qualifier != SpaceQualifier::Seq {"),),
     ((LIB, ("seq_claims_apply_only_to_program_jobs_on_seq_spaces",)),
      (suite("reified_seq"), ("reducer_seq_atomic_guard",)))),
    ("claim-before-success",
     "SQ-4: a claim holds as soon as it is made, before the job succeeds",
     ((EPHEMERAL,
       "        self.pending.insert(id.to_vec());\n"
       "        Ok(())\n",
       "        self.pending.insert(id.to_vec());\n"
       "        self.registry.store_claims(BTreeSet::from([id.to_vec()]), &self.path);\n"
       "        Ok(())\n"),),
     ((suite("reified_seq"), ("prop_seq_claim_chain",)),)),
    ("check-match-in-bind",
     "TH-6: the theory checks a datum in the spatial match, before the guard, and not after it",
     ((GENERIC,
       "    ) -> Result<Option<ListParWithRandom>, SpaceError> {\n"
       "        self.plan.matcher.bind(pattern, data)\n"
       "    }\n",
       "    ) -> Result<Option<ListParWithRandom>, SpaceError> {\n"
       "        let Some(bound) = self.plan.matcher.bind(pattern, data)? else {\n"
       "            return Ok(None);\n"
       "        };\n"
       "        match &self.plan.theory {\n"
       "            Some(theory) if !theory.check_match(data, &bound, self.meter())?.is_proven() => {\n"
       "                Ok(None)\n"
       "            }\n"
       "            Some(_) | None => Ok(Some(bound)),\n"
       "        }\n"
       "    }\n"),
      (GENERIC,
       "        let Some(matched) = self.plan.matcher.bind(pattern, data)? else {\n"
       "            return Ok(None);\n"
       "        };\n"
       "        let admitted = match held {\n"
       "            Held::Stored => self.admits(continuation, &matched, &[StoredMatch {\n"
       "                payload: data,\n"
       "                captures: &matched,\n"
       "            }])?,\n",
       "        let Some(matched) = self.bind(pattern, data)? else {\n"
       "            return Ok(None);\n"
       "        };\n"
       "        let admitted = match held {\n"
       "            Held::Stored => self.admits(continuation, &matched, &[])?,\n")),
     ((LIB, ("theory_match_reserves_one_unit_per_stored_datum",
             "space_guard_match_refusal_is_a_guard_refusal", "theory_nested_commit_guard")),)),
    ("admission-after-the-bump",
     "TH-5: the theory admits the values of a send after the counter bump",
     ((GENERIC,
       "        let admission = Admission::of(&config, &op, meter)?;\n"
       "        // SQ-3:",
       "        let deferred = Admission::of(&config, &op, meter);\n"
       "        let admission = Admission {\n"
       "            theory: deferred.as_ref().ok().and_then(|admission| admission.theory.clone()),\n"
       "            leases: Vec::new(),\n"
       "        };\n"
       "        // SQ-3:"),
      (GENERIC,
       "                    OperationAuthority::of_produce(&op, &introduction),\n"
       "                )?;\n"
       "                plan.bumped = bumped;\n",
       "                    OperationAuthority::of_produce(&op, &introduction),\n"
       "                )?;\n"
       "                plan.bumped = bumped;\n"
       "                plan.leases.extend(deferred?.leases);\n")),
     ((LIB, ("theory_admission_precedes_any_staging",)),)),
    ("hashset-payload-check-off",
     "HS-2: a HashSet collection takes a send with a payload",
     ((HASHSET,
       "            [_, ..] => Err(SpaceError::PayloadNotEmpty),",
       "            [_, ..] => Ok(()),"),),
     ((suite("reified_hashset"), ("reified_hashset_payload_not_empty",)),
      (suite("reified_pattern_enforcement"), ("pattern_enforcement_hashset_rows",)))),
    ("seq-allowed-in-legacy",
     "QT-5: legacy execution accepts the qualifier seq",
     ((CONSTRUCTOR,
       "            | (SpaceQualifier::Seq, Execution::Native) => {}",
       "            | (SpaceQualifier::Seq, _) => {}"),),
     ((LIB, ("reified_factory_",)),
      (suite("reified_temp_seq"), ("reducer_qualifier_matrix",)))),
)


def run_tests(targets: tuple[str, ...], filters: tuple[str, ...], log: Path) -> tuple[int, list[str]]:
    """Runs the tests `filters` of `targets` and returns the exit status and the failed tests."""
    command = ["cargo", "test", "-p", "rholang", *targets, "--", *filters]
    if os.environ.get("RRS_SANDBOX") != "1":
        command = ["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryMax=24G",
                   "-p", "CPUQuota=800%", "--", *command]
    tmp = REPO_ROOT / "target" / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    environment = {**os.environ, "TMPDIR": str(tmp), "CARGO_INCREMENTAL": "0",
                   "RUST_MIN_STACK": "8388608"}
    with log.open("w", encoding="utf-8") as handle:
        status = subprocess.run(command, cwd=REPO_ROOT, stdout=handle, stderr=subprocess.STDOUT,
                                env=environment, check=False).returncode
    text = log.read_text(encoding="utf-8")
    failed = [line.split()[1] for line in text.splitlines()
              if line.startswith("test ") and line.endswith("FAILED")]
    return status, failed


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--log-dir", type=Path,
                        default=REPO_ROOT / "target/verification/reified-rspaces/p7-mutants")
    parser.add_argument("--only", nargs="*", default=None, help="run only the named mutants")
    args = parser.parse_args()
    args.log_dir.mkdir(parents=True, exist_ok=True)
    selected = [m for m in MUTANTS if args.only is None or m[0] in args.only]
    files = {edit[0] for mutant in selected for edit in mutant[2]}
    originals = {path: path.read_text(encoding="utf-8") for path in files}
    killed = 0
    survived = 0
    try:
        for name, rule, edits, runs in selected:
            texts = dict(originals)
            stale = [str(path.relative_to(REPO_ROOT)) for path, old, _ in edits
                     if texts[path].count(old) != 1]
            if stale:
                survived += 1
                print(f"  FAIL {name}: stale in {', '.join(stale)}")
                continue
            for path, old, new in edits:
                texts[path] = texts[path].replace(old, new)
            for path in {edit[0] for edit in edits}:
                path.write_text(texts[path], encoding="utf-8")
            failures: list[str] = []
            statuses: list[int] = []
            try:
                for index, (targets, filters) in enumerate(runs):
                    status, failed = run_tests(targets, filters,
                                               args.log_dir / f"{name}-{index}.log")
                    statuses.append(status)
                    failures.extend(failed)
            finally:
                for path in {edit[0] for edit in edits}:
                    path.write_text(originals[path], encoding="utf-8")
            if failures:
                killed += 1
                print(f"  PASS {name} ({rule}): {len(failures)} tests fail: {', '.join(failures)}")
            else:
                survived += 1
                print(f"  FAIL {name} ({rule}): the mutant survives (exits {statuses})")
    finally:
        for path, text in originals.items():
            path.write_text(text, encoding="utf-8")
    restored = all(path.read_text(encoding="utf-8") == text for path, text in originals.items())
    print(f"p7 mutants: PASS={killed} FAIL={survived} restored={restored}")
    return 0 if survived == 0 and restored else 1


if __name__ == "__main__":
    sys.exit(main())
