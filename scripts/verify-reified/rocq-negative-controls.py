#!/usr/bin/env python3
"""Rocq negative controls for the Reified RSpaces P1 models.

Each control mutates one rule in a scratch copy of a module
(formal/rocq/reified_rspaces/theories). The mutation changes one definition,
one statement, or every site of a rule that the model states in more than
one definition. The control passes when the mutated copy fails to compile
and the first error is inside one of the expected declarations. So each
control shows that a proof depends on the rule that it models: the proof
breaks when the model or the claim reverts to the old rule.

The controls cover the round-2 rules and the round-3 rule
(prompts/p1-fix-rules.md):
  FR-1'  a canonical group is non-decreasing and can repeat a channel
         (H17, H22, the uniqueness of the canonical form)
  FR-5'  the parent tracker resets per evaluation scope, on a store swap
         and on a session change (H20)
  FR-9'  the sweep through the handle honors the nested-transaction guard
         (H13)
  FR-10  a produce-side peek fire never stores its incoming datum, and the
         agent re-offers that datum as the next operation (H11)
and two FR-1 controls that the round-2 rules keep (the sweep validates the
stored join groups, and a rejected read does not touch the cache).

Rules of the harness:
  * The pristine modules must compile first. Otherwise every control fails.
  * A control has one mutation, or a tuple of mutations that the harness
    applies in order. Each mutation text must occur exactly once in the
    module when the harness applies it. Otherwise the control fails as
    stale.
  * The scratch tree is under target/ and never in the source tree. TMPDIR
    points into it. Unless RRS_SANDBOX=1, each coqc run is bounded by a
    systemd user scope (MemoryMax, MemorySwapMax=0, CPUQuota).

Environment: RRS_SANDBOX, RRS_SCRATCH, RRS_MEMORY_MAX (default 6G),
RRS_CPU_QUOTA (default 400%), RRS_ROCQ_SRC (default formal/rocq/
reified_rspaces).

Exit codes: 0 every control passes, 1 a control failed, 2 usage or setup
error. Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# (id, module, description, old text, new text, expected first-failing
# declarations). A control that mutates more than one site gives a tuple of
# old texts and a tuple of new texts of the same length.
CONTROLS = (
    # ── FR-1' ────────────────────────────────────────────────────────────
    ("vs-strict-order", "VolatileSweep",
     "FR-1 strictness instead of FR-1': adjacent channels must differ",
     "y :: _ => chan_leb x y && nondecreasing t end",
     "y :: _ => chan_leb x y && negb (chan_leb y x) && nondecreasing t end",
     ("nondecreasing_sorted",)),
    ("vs-example-claims-fr1", "VolatileSweep",
     "the H22 example claims the FR-1 rule: [x; x] is not canonical",
     "    canonical [x; x] = true /\\ canonical [x; x; y] = true /\\\n"
     "    canonical [x; y; x] = false /\\ canonical [y; x; x] = false.\nProof.",
     "    canonical [x; x] = false /\\ canonical [x; x; y] = true /\\\n"
     "    canonical [x; y; x] = false /\\ canonical [y; x; x] = false.\nProof.",
     ("repeated_group_canonical_when_sorted",)),
    ("vs-unique-without-antisymmetry", "VolatileSweep",
     "the uniqueness lemma without the antisymmetry premise",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> leb_antisymmetric ->",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> True ->",
     ("sorted_unique_canonical",)),
    ("vs-weak-antisymmetry", "VolatileSweep",
     "an antisymmetry premise that concludes nothing",
     "Definition leb_antisymmetric : Prop :=\n"
     "  forall x y, chan_leb x y = true -> chan_leb y x = true -> x = y.",
     "Definition leb_antisymmetric : Prop :=\n"
     "  forall x y, chan_leb x y = true -> chan_leb y x = true -> True.",
     ("sorted_unique_canonical",)),
    ("vs-no-totality", "VolatileSweep",
     "a totality premise that holds for every comparison",
     "Definition leb_total : Prop :=\n"
     "  forall x y, chan_leb x y = true \\/ chan_leb y x = true.",
     "Definition leb_total : Prop :=\n"
     "  forall x y, chan_leb x y = true \\/ True.",
     ("isort_canonical",)),
    ("vs-keys-ignore-order", "VolatileSweep",
     "collision freedom that says nothing about two continuation keys",
     "    | CCont g1, CCont g2 => Permutation g1 g2",
     "    | CCont g1, CCont g2 => True",
     ("sorted_keys_injective",)),
    ("vs-sweep-skips-canonical-check", "VolatileSweep",
     "the sweep does not validate the order of the stored join groups (FR-1)",
     "  && forallb (fun g => is_none (tlookup (key_cont g) T)) G\n  && forallb canonical G.",
     "  && forallb (fun g => is_none (tlookup (key_cont g) T)) G.",
     ("validate_spec",)),
    # ── FR-9' ────────────────────────────────────────────────────────────
    ("vs-sweep-ignores-open-txn", "VolatileSweep",
     "the handle sweeps under an open transaction of the same task",
     "  if in_dec ctx_eq_dec cur open then (S, Some NestedTransaction)",
     "  if in_dec ctx_eq_dec cur open then\n"
     "    match sweep_volatile T installed S chans with\n"
     "    | Some S' => (S', None)\n"
     "    | None => (S, Some SweepRejected)\n"
     "    end",
     ("sweep_exact",)),
    # ── FR-1' in EventCorrelation.v ──────────────────────────────────────
    ("ec-check-rejects-repeats", "EventCorrelation",
     "the check rejects a repeated channel again (FR-1 DuplicateChannelInGroup)",
     "  if nondecreasing g then None else Some NonCanonicalChannelOrder.",
     "  if nondecreasing g && match g with [x; y] => negb (chan_leb y x) | _ => true end\n"
     "  then None else Some NonCanonicalChannelOrder.",
     ("canonical_nondecreasing",)),
    ("ec-h17-claims-fr1", "EventCorrelation",
     "H17 claims the FR-1 rule: a repeated channel fails",
     "  (forall c, chan_leb c c = true -> group_check [c; c] = None) /\\",
     "  (forall c, chan_leb c c = true -> group_check [c; c] = Some NonCanonicalChannelOrder) /\\",
     ("non_canonical_fails_closed",)),
    ("ec-unique-without-antisymmetry", "EventCorrelation",
     "the uniqueness lemma without the antisymmetry premise",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> leb_antisymmetric ->",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> True ->",
     ("sorted_unique_canonical",)),
    ("ec-read-caches-rejected-key", "EventCorrelation",
     "a rejected continuations read caches its key (FR-1 fails open)",
     "  | ARead (RConts g) =>\n      match group_check g with\n      | Some e => (cache, Fail (EGroup e))",
     "  | ARead (RConts g) =>\n      match group_check g with\n      | Some e => (touch (CCont g) cache, Fail (EGroup e))",
     ("access_grows",)),
    # ── FR-5' ────────────────────────────────────────────────────────────
    ("ec-scope-does-not-reset", "EventCorrelation",
     "begin_evaluation_scope keeps the tracker",
     "  | CallScope => Some (tracker_reset, [])",
     "  | CallScope => Some (tr, [])",
     ("two_evaluations_commit",)),
    ("ec-store-swap-ignored", "EventCorrelation",
     "the tracker ignores the store identity",
     "  | Some s, Some st => Nat.eqb s sess && Nat.eqb st store",
     "  | Some s, Some st => Nat.eqb s sess",
     ("scope_parents_other",)),
    ("ec-session-change-ignored", "EventCorrelation",
     "the tracker ignores the session",
     "  | Some s, Some st => Nat.eqb s sess && Nat.eqb st store",
     "  | Some s, Some st => Nat.eqb st store",
     ("scope_parents_other",)),
    ("ec-parent-reuse-admitted", "EventCorrelation",
     "a second commit under one parent in one scope is admitted",
     "      if in_dec path_eq_dec parent base then None\n      else Some",
     "      Some",
     ("run_calls_scope",)),
    ("ec-two-evaluations-claim-fr5", "EventCorrelation",
     "the two-evaluation lemma claims the FR-5 rule: the second evaluation fails after a scope reset",
     "                       CallScope; CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1 ++ sub_orders s parent n2 /\\\n"
     "    (forall st', st' <> st ->",
     "                       CallScope; CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1 /\\\n"
     "    (forall st', st' <> st ->",
     ("two_evaluations_commit",)),
    ("ec-no-reset-claims-commit", "EventCorrelation",
     "the two-evaluation lemma claims that a commit without a reset reuses the parent",
     "    snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);\n"
     "                       CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1.\nProof.",
     "    snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);\n"
     "                       CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1 ++ sub_orders s parent n2.\nProof.",
     ("two_evaluations_commit",)),
    # ── FR-10 ────────────────────────────────────────────────────────────
    ("ec-fire-stores-incoming", "EventCorrelation",
     "a peek fire stores its non-persistent incoming datum again (the P1 rule before FR-10) "
     "in the view and both commits",
     ("  let retain := negb (nullb (w_peeks wk)) in\n"
      "  vpf_loop V1 chs retain (sort_desc vkey cands).",
      "                  mkRes (fst p2) rd (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk",
      "                  mkRes (fst p2) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk"),
     ("  let retain := negb (nullb (w_peeks wk)) in\n"
      "  let V2 := vpf_loop V1 chs retain (sort_desc vkey cands) in\n"
      "  if retain && negb persist\n"
      "  then vput_datum V2 ch (mkDatum v persist (produce_id ch v persist)) else V2.",
      "                  let p3 := if retain && negb persist\n"
      "                            then put_datum (fst p2) ch (mkDatum v persist src) else (fst p2, []) in\n"
      "                  mkRes (fst p3) rd (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2 ++ snd p3) StOk",
      "                  let p3 := if retain && negb persist\n"
      "                            then put_datum (fst p2) ch (mkDatum v persist src) else (fst p2, []) in\n"
      "                  mkRes (fst p3) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2 ++ snd p3) StOk"),
     ("peek_fire_keeps_data",)),
    ("ec-kept-includes-incoming", "EventCorrelation",
     "the kept records of a produce-side peek fire include its incoming datum (the P1 retained)",
     "  | OProduce _ _ _ (PFire _ wk _ cands) =>\n"
     "      if negb (nullb (w_peeks wk)) then fun c => np_recs c cands else fun _ => []",
     "  | OProduce ch v persist (PFire _ wk _ cands) =>\n"
     "      if negb (nullb (w_peeks wk))\n"
     "      then fun c => np_recs c cands\n"
     "                    ++ (if negb persist && chan_eqb ch c\n"
     "                        then [mkDatum v persist (produce_id ch v persist)] else [])\n"
     "      else fun _ => []",
     ("retained_stored",)),
    ("ec-h11-claims-incoming-stored", "EventCorrelation",
     "H11 claims the P1 rule: a committed non-persistent produce-side peek fire stores its incoming datum",
     "  (forall m H S rd sg, retaining (sg_op sg) = true ->\n"
     "     forall c, s_data (r_st (commit_op m H S rd sg)) c = s_data S c) /\\",
     "  (forall m H S rd ch v chs wk kidx cands j0 scomm,\n"
     "     w_peeks wk <> [] ->\n"
     "     let r := commit_op m H S rd (mkStaged (OProduce ch v false (PFire chs wk kidx cands)) j0 scomm) in\n"
     "     r_status r = StOk ->\n"
     "     s_data (r_st r) ch = mkDatum v false (produce_id ch v false) :: s_data S ch) /\\",
     ("peek_retention_trace_eq_host",)),
    ("ec-h11-claims-p1-deviation", "EventCorrelation",
     "H11 claims the P1 accounting: the kept records replace every re-produce of HEAD, "
     "the incoming datum included",
     "  (forall op c, retaining op = true ->\n"
     "     values_on c (kept_reproduces (fire_cands op)) = map d_val (retained op c)) /\\",
     "  (forall op c, retaining op = true ->\n"
     "     values_on c (head_reproduces (fire_cands op)) = map d_val (retained op c)) /\\",
     ("peek_retention_trace_eq_host",)),
    ("ec-plan-stores-incoming", "EventCorrelation",
     "the head-only plan stores the datum after a peek fire (the P1 rule) instead of the re-offer",
     "            else if w_persist w then (ws, [])\n"
     "            else offer wmatch ws' [] d",
     "            else if negb (d_persist d) then ((if w_persist w then ws else ws'), [d])\n"
     "            else if w_persist w then (ws, [])\n"
     "            else offer wmatch ws' [] d",
     ("offer_keeps_no_pending_match",)),
    ("ec-reoffer-due-without-a10", "EventCorrelation",
     "the re-offer is due also after a persistent waiter (FR-10 without A10), "
     "which the plan cannot stage in the same transaction",
     "      negb persist && negb (nullb (w_peeks wk)) && negb (w_persist wk)",
     "      negb persist && negb (nullb (w_peeks wk))",
     ("offer_ops_reoffers",)),
)

DECLARATION = re.compile(
    r"^\s*(?:Lemma|Theorem|Corollary|Fact|Remark|Proposition|Definition|Fixpoint|Inductive|Record)\s+"
    r"([A-Za-z_][A-Za-z0-9_']*)\b")
ERROR_LOCATION = re.compile(r'^File "[^"]*", line (\d+), characters')


def bounded(command: list[str]) -> list[str]:
    if os.environ.get("RRS_SANDBOX") == "1":
        return command
    memory = os.environ.get("RRS_MEMORY_MAX", "6G")
    quota = os.environ.get("RRS_CPU_QUOTA", "400%")
    return ["systemd-run", "--user", "--scope", "--quiet", "-p", f"MemoryMax={memory}",
            "-p", "MemorySwapMax=0", "-p", f"CPUQuota={quota}", "--"] + command


def compile_module(directory: Path, module: str, env: dict[str, str]) -> tuple[int, str]:
    result = subprocess.run(bounded(["coqc", "-q", "-noglob", f"{module}.v"]), cwd=directory,
                            env=env, capture_output=True, text=True, check=False)
    return result.returncode, result.stdout + result.stderr


def enclosing_declaration(source: str, line: int) -> str | None:
    name = None
    for number, text in enumerate(source.split("\n"), start=1):
        if number > line:
            break
        match = DECLARATION.match(text)
        if match:
            name = match.group(1)
    return name


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--only", action="append", default=[], metavar="ID",
                        help="run only this control (repeatable)")
    parser.add_argument("--list", action="store_true", help="list the controls and exit")
    args = parser.parse_args()
    if args.list:
        for cid, module, description, *_ in CONTROLS:
            print(f"{cid:34s} {module:17s} {description}")
        return 0

    source_root = Path(os.environ.get("RRS_ROCQ_SRC", REPO_ROOT / "formal/rocq/reified_rspaces"))
    theories = source_root / "theories"
    scratch_root = Path(os.environ.get("RRS_SCRATCH",
                                       REPO_ROOT / "target/verification/reified-rspaces/scratch"))
    if os.environ.get("RRS_SANDBOX") == "1":
        log_dir = scratch_root / "logs" / "negative-controls"
    else:
        log_dir = REPO_ROOT / "target/verification/reified-rspaces/logs/negative-controls"
    scratch_root.mkdir(parents=True, exist_ok=True)
    log_dir.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="negctl.", dir=scratch_root))
    env = dict(os.environ)
    env["TMPDIR"] = str(work / "tmp")
    (work / "tmp").mkdir()

    selected = [c for c in CONTROLS if not args.only or c[0] in args.only]
    unknown = sorted(set(args.only) - {c[0] for c in CONTROLS})
    if unknown:
        print(f"error: unknown control(s): {', '.join(unknown)}", file=sys.stderr)
        return 2
    passed = failed = 0
    try:
        print(f"== Reified RSpaces Rocq negative controls ({len(selected)} controls)")
        print(f"   scratch: {work}")
        modules = sorted({c[1] for c in selected})
        pristine: dict[str, str] = {}
        for module in modules:
            path = theories / f"{module}.v"
            if not path.is_file():
                print(f"  FAIL baseline {module}: {path} is missing")
                return 1
            pristine[module] = path.read_text(encoding="utf-8")
            directory = work / "baseline" / module
            directory.mkdir(parents=True)
            (directory / f"{module}.v").write_text(pristine[module], encoding="utf-8")
            code, output = compile_module(directory, module, env)
            (log_dir / f"baseline-{module}.log").write_text(output, encoding="utf-8")
            if code != 0:
                print(f"  FAIL baseline {module}: the pristine module does not compile")
                print("\n".join("        " + line for line in output.strip().split("\n")[-10:]))
                return 1
            print(f"  PASS baseline {module}: the pristine module compiles")

        for cid, module, description, old, new, expected in selected:
            olds = old if isinstance(old, tuple) else (old,)
            news = new if isinstance(new, tuple) else (new,)
            if len(olds) != len(news):
                failed += 1
                print(f"  FAIL {cid}: malformed control, {len(olds)} old texts and {len(news)} new texts")
                continue
            mutated = pristine[module]
            stale = None
            for site, (old_text, new_text) in enumerate(zip(olds, news), start=1):
                count = mutated.count(old_text)
                if count != 1:
                    stale = (site, count)
                    break
                mutated = mutated.replace(old_text, new_text, 1)
            if stale is not None:
                failed += 1
                print(f"  FAIL {cid}: stale control, the mutation text of site {stale[0]} occurs "
                      f"{stale[1]} times in {module}.v")
                continue
            directory = work / cid
            directory.mkdir(parents=True)
            (directory / f"{module}.v").write_text(mutated, encoding="utf-8")
            code, output = compile_module(directory, module, env)
            (log_dir / f"{cid}.log").write_text(output, encoding="utf-8")
            if code == 0:
                failed += 1
                print(f"  FAIL {cid}: the mutant compiles ({description})")
                continue
            match = None
            for line in output.split("\n"):
                match = ERROR_LOCATION.match(line)
                if match:
                    break
            if match is None:
                failed += 1
                print(f"  FAIL {cid}: coqc failed with no error location; see {log_dir / (cid + '.log')}")
                continue
            line_number = int(match.group(1))
            where = enclosing_declaration(mutated, line_number)
            if where in expected:
                passed += 1
                print(f"  PASS {cid}: rejected in {where} (line {line_number}): {description}")
            else:
                failed += 1
                print(f"  FAIL {cid}: rejected in {where} (line {line_number}), expected one of "
                      f"{', '.join(expected)}")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print(f"reified-rspaces negative controls: PASS={passed} FAIL={failed}")
    print(f"   logs: {log_dir}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
