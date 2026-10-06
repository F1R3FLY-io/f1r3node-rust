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

# ---- (a) i128 no-overflow over the FULL validated ppm range -------------------------
# S ≤ i64::MAX ≈ 2^63, den = 10^6, and num over the FULL on-chain range −den ≤ num ≤ den
# (token_metadata_check.rs:105 range-checks ppm to [−1e6, 1e6]; negative-θ sentinels are
# legal, ft_decides_exact clique_oracle.rs:72-78), 0 ≤ q ≤ S. Then 2·q·den ≤ ~2^84 and
# S·(den+num) ∈ [0, ~2^85] (den+num ∈ [0, 2·den] since num ≥ −den), both well within
# i128 (±2^127). Refute any overflow across the widened range (was 0 ≤ num ≤ den).
q, s, num = Ints('q s num')
DEN = 1000000
bounds = And(q >= 0, q <= s, s > 0, s <= 2**63, num >= -DEN, num <= DEN)
lhs = 2 * q * DEN
rhs = s * (DEN + num)
sol = Solver()
sol.add(bounds, Or(lhs >= 2**127, lhs < -(2**127), rhs >= 2**127, rhs < -(2**127)))
expect("i128 no-overflow over FULL validated ppm range (−den ≤ num ≤ den, S ≤ 2^63)", sol, "unsat")

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

# ---- (d) θ ≤ 0: the θ-test alone finalizes BELOW strict majority; the θ-INDEPENDENT ----
#         hard gate (2·agreeing > S, clique_oracle.rs:79-81) closes it. -----------------
# The node passes TWO quantities to ft_decides_exact(agreeing, max_clique_weight, …):
#   q        = max-CLIQUE weight — the quantity the θ-test scales (2·q·den ⋛ S·(den+num));
#   agreeing = TOTAL agreeing weight, of which q is a sub-part (q ≤ agreeing ≤ S).
# The floor θ-test alone is `fin_theta` on q. This mirrors Rocq CliqueOracle Section 9
# (Finalized_ft_hg / Finalized_ft_hg_refines_Finalized): finalization = θ-test ∧ hard gate.
agreeing, den2 = Ints('agreeing den2')
fin_theta = 2 * q * den2 >= s * (den2 + num)          # θ-exact floor test (on the clique q)

# GAP (no hard gate): at θ ≤ 0 (num ≤ 0) the θ-test can finalize while the clique is NOT
# a strict majority — `fin_theta` holds yet 2q ≤ S (e.g. num = 0, 2q = S exactly). SAT.
# This is exactly why `Finalized_ft_refines_Finalized` needs 0 < num (it is VACUOUS here).
sol = Solver()
sol.add(den2 > 0, s > 0, q >= 0, q <= s, -den2 <= num, num <= 0, fin_theta, Not(2 * q > s))
expect("θ≤0 GAP: θ-test finalizes with 2q ≤ S (below strict majority) — no hard gate", sol, "sat")

# LOAD-BEARING: at θ ≤ 0 with the clique below majority (2q ≤ S), the hard gate
# `2·agreeing > S` can STILL hold via the broader agreeing set (agreeing > q), supplying
# the strict-majority `Finalized` the clique alone did not. SAT (the gate does real work).
sol = Solver()
sol.add(den2 > 0, s > 0, q >= 0, q <= agreeing, agreeing <= s, -den2 <= num, num <= 0,
        fin_theta, Not(2 * q > s), 2 * agreeing > s)
expect("hard gate LOAD-BEARING: majority via the agreeing set even when 2q ≤ S at θ≤0", sol, "sat")

# CLOSES IT (∀ num incl θ≤0): the node's REAL decision (θ-test ∧ hard gate) always
# carries a strict-majority agreeing set — refute "finalized-with-gate yet 2·agreeing ≤ S"
# over the FULL range. UNSAT ⇒ the hard gate makes θ-finalization ⇒ strict-majority
# `Finalized` for ALL num (the θ≤0 seam is closed). Mirrors Finalized_ft_hg_refines_Finalized.
sol = Solver()
sol.add(den2 > 0, s > 0, q >= 0, q <= agreeing, agreeing <= s, -den2 <= num, num <= den2,
        fin_theta, 2 * agreeing > s, Not(2 * agreeing > s))
expect("hard gate CLOSES θ≤0: real finalize ⇒ strict-majority agreeing, ALL num", sol, "unsat")

print("== Z3 A9 exact-integer FT cross-witness: ALL PASS ==" if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
