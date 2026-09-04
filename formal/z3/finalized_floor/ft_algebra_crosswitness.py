#!/usr/bin/env python3
# Z3 cross-witness for the finalized-floor numeric claims that underlie the Rocq
# theorems (CliqueOracle.L_ANC / L_SNAP, Merge determinism) and the A9 exact-
# integer FT-threshold hardening. Independent confirmation = the multi-prover gate
# (Rocq authoritative; Z3, Sage, and Wolfram independent cross-witnesses). Each
# claim is proved by showing its negation is UNSAT.
from z3 import *

ok = True
def prove(name, claim):
    global ok
    s = Solver(); s.add(Not(claim)); r = s.check()
    if r == unsat: print(f"  PASS  {name}")
    else: print(f"  FAIL  {name}: {r} {s.model() if r==sat else ''}"); ok = False

# 1. A9: the strict ratio comparison (2q-S)/S > num/den is EXACTLY the integer
#    comparison 2q*den > S*(den+num) for S>0, den>0 — the exact-arithmetic
#    hardening is faithful.
qr,Sr,numr,denr = Reals('qr Sr numr denr')
prove("A9 strict FT ratio == cross-multiplied form (reals, S>0, den>0)",
      Implies(And(Sr>0, denr>0),
              ((2*qr-Sr)/Sr > numr/denr) == ((2*qr-Sr)*denr > numr*Sr)))
q,S,num,den = Ints('q S num den')
prove("A9 strict cross-mult == 2q*den > S*(den+num) (ints, S>0, den>0)",
      Implies(And(S>0, den>0),
              ((2*q-S)*den > num*S) == (2*q*den > S*(den+num))))
prove("inclusive arithmetic control remains equivalent",
      Implies(And(S>0, den>0),
              ((2*q-S)*den >= num*S) == (2*q*den >= S*(den+num))))

# 2. L-ANC (arithmetic core): agreeing weight of an ancestor >= that of a
#    descendant (from agreeing-set inclusion, proven in Rocq); if the descendant
#    is finalized (2*aw > S) then so is the ancestor.
awA,awD,Sw = Ints('awA awD Sw')
prove("L-ANC weight-monotone: awA>=awD & 2awD>S => 2awA>S",
      Implies(And(awA>=awD, 2*awD>Sw), 2*awA>Sw))

# 3. L-SNAP (arithmetic core): a larger snapshot only grows the agreeing weight.
awS,awB = Ints('awS awB')
prove("L-SNAP weight-monotone: awB>=awS & 2awS>S => 2awB>S",
      Implies(And(awB>=awS, 2*awS>Sw), 2*awB>Sw))

# 4. T-DETMERGE core: the keep-one (max) semilattice fold is order-independent
#    over 3 elements (all 6 permutations agree).
a,b,c = Ints('a b c')
mx = lambda x,y: If(x>=y,x,y)
P = [mx(a,mx(b,c)),mx(a,mx(c,b)),mx(b,mx(a,c)),mx(b,mx(c,a)),mx(c,mx(a,b)),mx(c,mx(b,a))]
prove("keep-one (max) 3-fold order-independent", And(*[P[0]==p for p in P[1:]]))

# 5. T-K1 core: max fold absorbs every input (winner dominates each candidate).
prove("keep-one winner >= each input (3 elts)",
      And(a<=mx(a,mx(b,c)), b<=mx(a,mx(b,c)), c<=mx(a,mx(b,c))))

print("== Z3 cross-witness: ALL PASS ==" if ok else "== Z3 cross-witness: FAILURES ==")
import sys; sys.exit(0 if ok else 1)
