# Sage cross-witness (symbolic CAS) of the finalized-floor FT-threshold algebra
# and finalization monotonicity — the third leg of the multi-prover gate
# (Rocq authoritative; Sage, Z3, and Wolfram independent cross-witnesses)
# confirming the L-ANC/L-SNAP algebra.
var('q S num den awA awD')

# 1. A9 exact-integer FT-threshold equivalence. Multiplying (2q-S)/S > num/den
#    by S*den > 0 is order-preserving, so the ratio test equals the integer test
#    2q*den > S*(den+num); the algebraic margins are identical after clearing denoms.
identity = ((2*q - S)*den - num*S) - (2*q*den - S*(den + num))
id0 = identity.simplify_full()
print("A9: (2q-S)*den - num*S  -  (2q*den - S*(den+num))  =", id0, "(expect 0)")
assert id0 == 0, "FT cross-multiplication is NOT exact"

# 2. L-ANC / L-SNAP monotonicity: the finalized margin m(aw) = 2*aw - S is
#    monotone increasing in the agreeing weight aw, so awA >= awD implies
#    m(awA) >= m(awD): if the descendant/smaller-snapshot side is finalized
#    (m > 0) the ancestor/larger-snapshot side is too.
margin_gap = (2*awA - S) - (2*awD - S)
mg = margin_gap.simplify_full()
print("L-ANC/L-SNAP: m(awA) - m(awD) =", mg, "= 2*(awA-awD) >= 0 when awA>=awD")
assert mg == 2*(awA - awD), "finalized margin is not the expected linear form"

# 3. A9 exact-integer finalization DECISION (the f32 -> exact hardening). The exact test
#    `2*q*den > S*(den+num)` is the cleared-denominator form of `(2q-S)/S > num/den`
#    (multiply both sides by S*den > 0, order-preserving), so it is EXACT — no f32 fuzz.
#    Confirms Rocq FtExact.v `ft_exact_iff_ratio_strict`; the inclusive historical
#    control shares the same algebraic identity but is not the node decision.
exact_minus_ratio = (2*q*den - S*(den + num)) - ((2*q - S)*den - num*S)
er = exact_minus_ratio.simplify_full()
print("A9 exact==ratio: (2q*den - S(den+num)) - ((2q-S)den - num*S) =", er, "(expect 0)")
assert er == 0, "exact-integer FT test is not the cleared-denominator ratio test"
# The boundary tie `2q*den == S(den+num)` is exactly `(2q-S)/S == num/den` (at threshold):
# `(2q-S)*den - num*S == 0` iff the ratio equals theta, so the node's strict test rejects it.
tie = ((2*q - S)*den - num*S)
print("A9 boundary: (2q-S)*den - num*S =", tie.simplify_full(),
      "= 0 iff (2q-S)/S == num/den (strict node decision rejects)")

print("== Sage cross-witness: ALL PASS ==")
