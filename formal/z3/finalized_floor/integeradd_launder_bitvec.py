#!/usr/bin/env python3
# Z3 BitVec-64 cross-witness for the IntegerAdd overflow-launder (W2, machine-int
# fidelity leg). bvadd on BitVec 64 IS Rust's i64 wrapping_add (2's-complement mod
# 2^64), so this confirms the Rocq Z-mod model (IntegerAdd.v) against exact machine
# semantics. Signed comparisons via SignExt to 128 bits give the "true" (unbounded)
# arithmetic.
from z3 import *

ok = True
def expect(name, solver, want):  # want in {"sat","unsat"}
    global ok
    r = solver.check()
    good = (str(r) == want)
    print(f"  {'PASS' if good else 'FAIL'}  {name}: {r} (expected {want})")
    if good and want == "sat":
        print(f"        witness: {solver.model()}")
    ok = good and ok

MAX128 = BitVecVal(2**63 - 1, 128)
MIN128 = BitVecVal(-2**63, 128)
def ext(x): return SignExt(64, x)                       # 64 -> 128, signed
def in_range(x128): return And(x128 <= MAX128, x128 >= MIN128)

# ---- (i) EXHIBIT: the buggy wrapping path launders past the checked_add >=0 gate.
d1, d2, d3, base = BitVecs('d1 d2 d3 base', 64)
wsum   = d1 + d2 + d3                                    # wrapping combine (merging_logic.rs:42)
tsum   = ext(d1) + ext(d2) + ext(d3)                    # true (no-wrap) sum
applied_wrap = ext(base) + ext(wsum)                    # base + wrapped combine
applied_true = ext(base) + tsum                         # base + true sum
gate = And(in_range(applied_wrap), applied_wrap >= 0)   # the checked_add >=0 apply gate ACCEPTS
s = Solver()
s.add(gate)                                             # buggy gate accepts, AND
s.add(Not(in_range(applied_true)))                      # the TRUE result overflows i64 -> should reject
expect("launder EXISTS on wrapping path (bvadd)", s, "sat")

# ---- (ii) FIX is launder-free: checked_combine rejects on any step overflow, so if
# it ACCEPTS, no wrap occurred and the wrapping sum equals the true sum.
# checked step: bvadd of two in-range operands with no signed overflow.
def add_ok(a128, b128):
    r = a128 + b128
    return And(in_range(a128), in_range(b128), in_range(r))
# checked_combine [d1,d2,d3] accepts  <=>  each partial (right-fold) stays in range.
p1 = ext(d3)                                            # true_sum [d3]
p2 = ext(d2) + p1                                       # true_sum [d2,d3]
p3 = ext(d1) + p2                                       # true_sum [d1,d2,d3]
checked_accepts = And(in_range(p1), in_range(p2), in_range(p3))
# claim: checked_accepts  =>  wrapping combine == true sum (no launder). Refute the negation.
s2 = Solver()
s2.add(checked_accepts)
s2.add(ext(wsum) != tsum)                               # wrapped != true  (a laundered value)
expect("checked_combine is launder-free (bvadd)", s2, "unsat")

# ---- (iii) checked accept => the value is exactly the true sum, in range.
s3 = Solver()
s3.add(checked_accepts)
s3.add(Or(Not(in_range(tsum)), ext(wsum) != tsum))
expect("checked accept => true sum, in range", s3, "unsat")

print("== Z3 BitVec-64 IntegerAdd cross-witness: ALL PASS ==" if ok else "== FAILURES ==")
import sys; sys.exit(0 if ok else 1)
