#!/usr/bin/env python3
# Z3 BitVec-64 cross-witness for fork-choice SCORE accumulation
# (estimator::build_scores_map, estimator.rs:249-259). Validator weights are
# non-negative i64 and each block's cumulative score is a SUM of the weights of the
# validators whose latest message supports it, bounded by the total bonded stake
# (the supply cap). bvadd on BitVec 64 IS Rust's i64 wrapping add, so this confirms,
# against exact machine semantics:
#   (i)  score accumulation is associative & commutative — the HashMap iteration
#        order over validators cannot change any score (Rocq Score.score_perm_invariant);
#   (ii) while the running sum stays <= cap < 2^63 there is NO overflow, so the B3
#        checked_add only ever fires on a supply-cap violation (already-invalid state);
#   (iii) above the cap the wrap is real (a wrong value), which is why B3 uses
#        checked_add rather than a silent `+=`.
from z3 import *

ok = True
def expect(name, s, want):
    global ok
    r = s.check()
    good = (str(r) == want)
    print(f"  {'PASS' if good else 'FAIL'}  {name}: {r} (expected {want})")
    if good and want == "sat":
        print(f"        witness: {s.model()}")
    ok = good and ok

a, b, c = BitVecs('a b c', 64)

# (i) associativity + commutativity of bvadd  (order-independent scores)
s = Solver(); s.add((a + b) + c != a + (b + c))
expect("score add associative", s, "unsat")
s = Solver(); s.add(a + b != b + a)
expect("score add commutative", s, "unsat")

# (ii) non-negative weights, every running (prefix) sum <= cap < 2^63  =>  no signed
#      overflow, so the wrapping sum equals the true (unbounded) sum.
MAX128 = BitVecVal(2**63 - 1, 128)
CAP128 = BitVecVal(2**62, 128)          # a supply cap safely below i64::MAX
def ext(x): return SignExt(64, x)       # 64 -> 128, signed
nonneg = And(a >= 0, b >= 0, c >= 0)    # signed i64 >= 0
p1 = ext(a)
p2 = ext(a) + ext(b)
p3 = ext(a) + ext(b) + ext(c)
under_cap = And(p1 <= CAP128, p2 <= CAP128, p3 <= CAP128)
s = Solver(); s.add(nonneg, under_cap, ext(a + b + c) != p3)
expect("no overflow while every prefix sum <= cap (wrapping == true)", s, "unsat")

# (iii) EXHIBIT: two non-negative weights whose true sum exceeds i64::MAX wrap to a
#       wrong (out-of-true-range) value — motivating the B3 checked_add.
s = Solver(); s.add(a >= 0, b >= 0, ext(a) + ext(b) > MAX128, ext(a + b) != ext(a) + ext(b))
expect("overflow EXISTS above the cap (wrap corrupts the score)", s, "sat")

print("== Z3 fork-choice score supply-cap cross-witness: ALL PASS =="
      if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
