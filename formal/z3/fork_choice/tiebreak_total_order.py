#!/usr/bin/env python3
# Z3 cross-witness for the fork-choice tie-break TOTAL ORDER — the S1 / no-fork
# determinism linchpin. The estimator ranks tips with
# ListOps::sort_by_with_decreasing_order (shared/src/rust/shared/list_ops.rs:44-67):
# block SCORE descending, then block HASH ascending. On DISTINCT hashes this
# "precedes" relation is a STRICT TOTAL ORDER, so the argmax (the chosen main tip) is
# UNIQUE and independent of the HashSet/HashMap iteration order upstream in the
# estimator — i.e. NO cross-node fork. This confirms Rocq TieBreak.sort_total_order /
# output_indep_of_input_perm; the TLA+ MC_ForkChoice_nontotal.cfg is the dual
# counterexample where breaking totality reintroduces the fork.
from z3 import *

ok = True
def expect(name, s, want):  # want in {"sat","unsat"}
    global ok
    r = s.check()
    good = (str(r) == want)
    print(f"  {'PASS' if good else 'FAIL'}  {name}: {r} (expected {want})")
    if good and want == "sat":
        print(f"        witness: {s.model()}")
    ok = good and ok

# entry = (score, hash).  "a precedes b" in the fork-choice ranking:
#   higher score first, then (on equal score) lower hash first.
def prec(sa, ha, sb, hb):
    return Or(sa > sb, And(sa == sb, ha < hb))

sa, ha, sb, hb, sc, hc = Ints('sa ha sb hb sc hc')

# irreflexive: nothing precedes itself
s = Solver(); s.add(prec(sa, ha, sa, ha))
expect("irreflexive", s, "unsat")

# asymmetric: prec(a,b) and prec(b,a) cannot both hold
s = Solver(); s.add(prec(sa, ha, sb, hb), prec(sb, hb, sa, ha))
expect("asymmetric", s, "unsat")

# transitive: prec(a,b) and prec(b,c) implies prec(a,c) — refute the negation
s = Solver(); s.add(prec(sa, ha, sb, hb), prec(sb, hb, sc, hc), Not(prec(sa, ha, sc, hc)))
expect("transitive", s, "unsat")

# total on DISTINCT hashes: ha != hb implies prec(a,b) or prec(b,a) — refute negation
s = Solver(); s.add(ha != hb, Not(Or(prec(sa, ha, sb, hb), prec(sb, hb, sa, ha))))
expect("total on distinct hashes", s, "unsat")

# argmax uniqueness: two DISTINCT entries cannot BOTH be maximal (an entry is maximal
# iff nothing precedes it). By totality on distinct hashes one must precede the other,
# so the chosen tip is unique — no iteration-order tie-break, no fork.
s = Solver(); s.add(ha != hb, Not(prec(sb, hb, sa, ha)), Not(prec(sa, ha, sb, hb)))
expect("argmax unique on distinct hashes", s, "unsat")

print("== Z3 fork-choice tie-break total-order cross-witness: ALL PASS =="
      if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
