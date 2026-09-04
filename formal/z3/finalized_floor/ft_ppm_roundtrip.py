#!/usr/bin/env python3
# Z3 cross-witness for G2 — provenance & round-trip of the θ fault-tolerance
# threshold ppm, and the invariance of the EXACT finalization decision under the
# f32 display path.
#
# The node sources θ as an on-chain integer ppm numerator (θ = num/1e6), then runs
# the A9 exact-integer decision `2·q·den ⋛ S·(den+num)` (den = 1e6). The ONLY place
# an f32 appears is the DISPLAY conversion `to_ppm(x) = round(f64(x)·1e6)`
# (ProofOfStake::fault_tolerance_threshold_to_ppm) and its inverse
# `from_ppm(n) = (n/1e6) as f32`. This witnesses, against exact machine arithmetic
# and IEEE-754:
#   (1) to_ppm is MONOTONE in x on the θ-domain [-1, 1];
#   (2) the display round-trip is tight: |from_ppm(to_ppm(x)) - x| ≤ 0.5/1e6 + f32_eps;
#   (3) to_ppm(x) ∈ [-1e6, 1e6] — exactly the runtime range-check
#       (token_metadata_check.rs:105);
#   (4) once `num` is the integer ppm, the exact decision is INVARIANT under the
#       f32→ppm→f32 display path — redisplay(num) = num (a fixed point), so the
#       display conversion can never perturb the consensus decision.
# Confirms the Rocq FtProvenance.v lemmas (reconcile_is_onchain, agreement, and the
# widened ppm_range_decision_no_overflow over num ∈ [-den, den]).
#
# ROUNDING MODEL. Because x is an f32 in [-1,1] (≤24 significant bits) and 1e6 < 2^20,
# the f64 product f64(x)·1e6 needs ≤44 significand bits and is EXACT in f64 (53-bit
# mantissa) — a single rounding, so to_ppm(x) = nearest_int(real(x)·1e6). We drive the
# GENERAL properties (1)-(4) symbolically in exact real/integer arithmetic (decisive
# LIRA), modelling nearest-int as round-half-up `floor(y + 1/2)`; the tie rule is
# irrelevant to every coarse (½-ULP) bound below, and Rust's `f64::round` (ties away)
# and IEEE `fpRoundToIntegral(RNE)` (ties even) both satisfy |n − y| ≤ 1/2. A CONCRETE
# Float32/Float64 + fpRoundToIntegral(RNE) block corroborates the model against the
# real IEEE semantics (symbolic Int→Real→FP search is unreliable — see
# ft_exact_no_overflow.py — but CONCRETE FP is decisive).
#
# REPARAMETRIZATION. The general checks work in the exact reparametrization
# `u := x·1e6 ∈ [-1e6, 1e6]`, so to_ppm = round(u) = floor(u + 1/2) has UNIT
# coefficients. (Encoding the floor of `x·1e6` directly gives Z3 a modulus-1e6
# integer problem that does not terminate; the u-domain is arithmetically identical
# and decidable in milliseconds.) The x-domain round-trip bound |·| ≤ 0.5/1e6 + eps
# is multiplied through by 1e6 to the scaled bound |n + δ·1e6 − u| ≤ 1/2 + eps·1e6.
from z3 import *
import struct

ok = True
def expect(name, s, want):  # want in {"sat","unsat"}
    global ok
    s.set("timeout", 20000)          # safety net: never hang the gate; unknown ⇒ FAIL
    r = s.check()
    good = (str(r) == want)
    print(f"  {'PASS' if good else 'FAIL'}  {name}: {r} (expected {want})")
    if good and want == "sat":
        print(f"        witness: {s.model()}")
    ok = good and ok

MILL = 1000000                 # den = 1e6 (ppm denominator)
HALF = Q(1, 2)                 # 1/2
EPS  = Q(1, 2**24)             # f32 max round-to-nearest error for |v| ≤ 1 (½ ULP@1)
E    = Q(MILL, 2**24)          # f32_eps scaled by 1e6 (≈ 0.0596): |δ·1e6| bound

def is_round_half_up(y, n):
    # n == floor(y + 1/2), i.e. round-to-nearest-integer (ties up): n ≤ y+1/2 < n+1.
    return And(ToReal(n) <= y + HALF, y + HALF < ToReal(n) + 1)

# ---- (1) to_ppm monotone in x on [-1,1]  (u = x·1e6 ∈ [-1e6,1e6]) ------------------
u1, u2 = Reals('u1 u2'); n1, n2 = Ints('n1 n2')
sol = Solver()
sol.add(-MILL <= u1, u1 <= MILL, -MILL <= u2, u2 <= MILL)
sol.add(is_round_half_up(u1, n1), is_round_half_up(u2, n2))
sol.add(u1 <= u2, n1 > n2)                       # a monotonicity VIOLATION
expect("to_ppm monotone in x on the θ-domain [-1,1]", sol, "unsat")

# ---- (3) to_ppm(x) ∈ [-1e6, 1e6] (runtime range-check) ----------------------------
u = Real('u'); n = Int('n')
sol = Solver()
sol.add(-MILL <= u, u <= MILL, is_round_half_up(u, n))
sol.add(Or(n < -MILL, n > MILL))                 # out-of-range VIOLATION
expect("to_ppm(x) ∈ [-1e6, 1e6] on x ∈ [-1,1]", sol, "unsat")

# ---- (2) display round-trip within ½ppm + f32_eps ---------------------------------
# x-domain: |from_ppm(to_ppm(x)) - x| ≤ 0.5/1e6 + f32_eps. Scaled by 1e6 (u = x·1e6,
# n = to_ppm(x), δ = f32 cast slack, Dd = δ·1e6): |n + Dd - u| ≤ 1/2 + f32_eps·1e6.
u = Real('u'); n = Int('n'); Dd = Real('Dd')
sol = Solver()
sol.add(-MILL <= u, u <= MILL, is_round_half_up(u, n))   # n = to_ppm(x)
sol.add(-E <= Dd, Dd <= E)                               # f32 cast slack, scaled
B = HALF + E
rt = ToReal(n) + Dd                                      # 1e6 · from_ppm(to_ppm(x))
sol.add(Or(rt - u > B, u - rt > B))                      # bound VIOLATION
expect("|from_ppm(to_ppm(x)) - x| ≤ 0.5/1e6 + f32_eps", sol, "unsat")

# ---- (4a) redisplay(num) = num — the display path is a fixed point on integer ppm --
# redisplay(num) = to_ppm(from_ppm(num)); from_ppm(num) = num/1e6 + δ (|δ| ≤ f32_eps),
# so 1e6·from_ppm(num) = num + δ·1e6 = num + Dd, |Dd| ≤ 1e6/2^24 ≈ 0.0596 < 1/2, hence
# floor(num + Dd + 1/2) = num. Refute any num where redisplay(num) ≠ num.
num = Int('num'); Dd = Real('Dd'); m = Int('m')
sol = Solver()
sol.add(-MILL <= num, num <= MILL, -E <= Dd, Dd <= E)
sol.add(is_round_half_up(ToReal(num) + Dd, m))           # m = redisplay(num)
sol.add(m != num)
expect("redisplay(num) = num (f32→ppm→f32 is a fixed point on integer ppm)", sol, "unsat")

# ---- (4b) exact decision invariant under the f32→ppm→f32 display path --------------
# The decision is a pure function of the integer ppm; with redisplay(num) = num (4a),
# replacing num by its redisplayed value cannot change 2q·den ⋛ S(den+num). Refute any
# (q,S,num) where the decision at num disagrees with the decision at m = redisplay(num).
q, S, num, m = Ints('q S num m')
den = MILL
sol = Solver()
sol.add(0 <= q, q <= S, 0 < S, S <= 2**63, -MILL <= num, num <= MILL)
sol.add(m == num)                                        # 4a: redisplay(num) = num
dec_num = 2 * q * den > S * (den + num)
dec_m   = 2 * q * den > S * (den + m)
sol.add(dec_num != dec_m)                                # decision DISAGREEMENT
expect("exact decision invariant under f32→ppm→f32 display path", sol, "unsat")

# ---- (FP) concrete IEEE Float32/Float64 + fpRoundToIntegral(RNE) corroboration -----
# Confirms the exact-real model above matches the actual IEEE conversion the runtime
# performs, on representative f32 inputs. For x ∈ [-1,1] the f64 multiply is exact, so
# fpRoundToIntegral(RNE, f64(x)·1e6) = round-ties-even(f32(x)·1e6); we check equality
# with the value computed independently in Python (struct → f32, IEEE f64 multiply).
rne = RNE()
def f32(v):  # nearest IEEE Float32 to v, returned as a Python float (f64)
    return struct.unpack('<f', struct.pack('<f', v))[0]

fp_points = [0.0, 1.0, -1.0, 0.5, -0.5, 0.25, 0.75, -0.75, 0.1, -0.1, 0.333333, 0.9]
fp_ok = True
for v in fp_points:
    fv = f32(v)
    expected = round(fv * 1e6)                            # IEEE f64 multiply, ties-even
    x32  = FPVal(v, Float32())                            # nearest f32 to v
    x64  = fpToFP(rne, x32, Float64())                    # widen (exact)
    prod = fpMul(rne, x64, FPVal(1000000.0, Float64()))
    rnd  = fpRoundToIntegral(rne, prod)                   # to_ppm via IEEE
    s = Solver(); s.set("timeout", 20000)
    s.add(fpEQ(rnd, FPVal(float(expected), Float64())))
    if str(s.check()) != "sat":
        fp_ok = False
        print(f"  FAIL  IEEE to_ppm({v}) matches model → {expected}: {s.check()}")
    if not (-MILL <= expected <= MILL):
        fp_ok = False
        print(f"  FAIL  IEEE to_ppm({v}) = {expected} out of [-1e6,1e6]")
print(f"  {'PASS' if fp_ok else 'FAIL'}  IEEE Float32/Float64 fpRoundToIntegral(RNE) matches model on {len(fp_points)} points, all in range")
ok = fp_ok and ok

print("== Z3 G2 ppm provenance + round-trip cross-witness: ALL PASS ==" if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
