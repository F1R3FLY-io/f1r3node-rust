#!/usr/bin/env python3
# Z3 cross-witness for A9 — the exact-integer fault-tolerance DECISION that replaces
# the f32 comparison in the clique oracle (clique_oracle.rs::ft_decides_exact).
#
# The oracle finalizes iff (2q − S)/S ≥ θ, θ = num/den (q = max-clique weight,
# S = total bonded stake, θ = ppm/1_000_000). Cleared of denominators (S, den > 0):
#   2·q·den ≥ S·(den + num).
# This witnesses, against exact machine integers + IEEE-754:
#   (a) the i128 products never overflow for realistic bonds (S ≤ i64::MAX, den = 10^6);
#   (b) the exact test is EXACTLY the cleared-denominator rational test (all integers);
#   (c) the f32 residual is REAL — the i64→f32 cast is non-injective above 2^24, so a
#       decision computed in f32 cannot distinguish stakes that the exact test does
#       (this is precisely the precision fuzz A9 removes).
# Confirms the Rocq FtExact.v lemmas (ft_exact_iff_ratio, ft_exact_no_overflow).
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

# ---- (a) i128 no-overflow for the declared bounds ----------------------------------
# S ≤ i64::MAX ≈ 2^63, den = 10^6, 0 ≤ num ≤ den, 0 ≤ q ≤ S. Then 2·q·den ≤ ~2^84 and
# S·(den+num) ≤ ~2^84, both well within i128 (±2^127). Refute any overflow.
q, s, num = Ints('q s num')
DEN = 1000000
bounds = And(q >= 0, q <= s, s > 0, s <= 2**63, num >= 0, num <= DEN)
lhs = 2 * q * DEN
rhs = s * (DEN + num)
sol = Solver()
sol.add(bounds, Or(lhs >= 2**127, lhs < -(2**127), rhs >= 2**127, rhs < -(2**127)))
expect("i128 no-overflow for realistic bonds (S ≤ 2^63, den = 10^6)", sol, "unsat")

# ---- (b) exact test == cleared-denominator ratio test (exact integers) -------------
den = Int('den')
exact_ge = 2 * q * den >= s * (den + num)          # ft_exact_ge
ratio_ge = (2 * q - s) * den >= num * s            # (2q−S)/S ≥ num/den, cleared (S,den>0)
sol = Solver()
sol.add(s > 0, den > 0, exact_ge != ratio_ge)
expect("exact test ≡ cleared-denominator ratio test (all integers)", sol, "unsat")
# strict twin (for the LFB finalizer's `>`):
exact_gt = 2 * q * den > s * (den + num)
ratio_gt = (2 * q - s) * den > num * s
sol = Solver()
sol.add(s > 0, den > 0, exact_gt != ratio_gt)
expect("exact strict test ≡ cleared-denominator strict ratio test", sol, "unsat")

# ---- (c) the f32 residual is real: i64→f32 is non-injective above 2^24 -------------
# Two DISTINCT i64 stakes — 2^24 and 2^24+1 — round to the SAME Float32 (RNE ties 2^24+1
# down to 2^24, whose significand LSB is even), so a finalization decision taken in f32
# cannot distinguish them — precisely the precision fuzz A9 removes. (z3's SYMBOLIC
# Int→Real→FP search is unreliable across the Int/Real/FP theory combination; the
# CONCRETE collision below is decisive — verified: fpEQ holds.)
rne = RNE()
f_lo = fpToFP(rne, ToReal(IntVal(2**24)),     Float32())
f_hi = fpToFP(rne, ToReal(IntVal(2**24 + 1)), Float32())
sol = Solver()
sol.add(fpEQ(f_lo, f_hi))
expect("i64→f32 collides at 2^24 vs 2^24+1 (f32 decision residual is real)", sol, "sat")

print("== Z3 A9 exact-integer FT cross-witness: ALL PASS ==" if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
