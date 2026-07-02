# Sage cross-witness (symbolic CAS) of the finalized-floor FT-threshold algebra
# and finalization monotonicity — the third leg of the multi-prover gate
# (Wolfram primary; Z3 + Sage cross-witness) confirming the Rocq L-ANC/L-SNAP.
var('q S num den awA awD')

# 1. A9 exact-integer FT-threshold equivalence. Multiplying (2q-S)/S >= num/den
#    by S*den > 0 is order-preserving, so the ratio test equals the integer test
#    2q*den >= S*(den+num) iff the two sides differ by 0 after clearing denoms.
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

print("== Sage cross-witness: ALL PASS ==")
