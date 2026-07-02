#!/usr/bin/env python3
# Z3 cross-witness for GAP-1: the per-channel `ChannelChange.combine` algebra
# (rspace++/src/rspace/merger/channel_change.rs). A per-datum change is modeled
# as its (added, removed) multiplicity pair. `combine` performs
#     added   = vec_union(added)      (idempotent MAX-multiset union)
#     removed = vec_union(removed)
#     cancel_common(added, removed)   (min-multiplicity cancellation)
# This confirms Rocq ChannelNetting.combine_max_comm (commutativity HOLDS),
# combine_not_assoc_exhibit (Finding A: max-union is NON-associative), and
# channel_netting_fixed_deterministic (the sum-union FIX restores associativity,
# hence an order-independent netted diff).
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

def zmin(x, y): return If(x <= y, x, y)
def zmax(x, y): return If(x >= y, x, y)

# a change is a pair (added, removed) of non-negative multiplicities.
def cancel(ch):
    a, r = ch
    m = zmin(a, r)
    return (a - m, r - m)

# SHIPPED max-union combine (buggy): per-side max, then cancel_common.
def cmax(x, y):
    return cancel((zmax(x[0], y[0]), zmax(x[1], y[1])))

# FIXED sum-union combine: componentwise addition (cancel deferred).
def csum(x, y):
    return (x[0] + y[0], x[1] + y[1])

def neq(u, v):
    return Or(u[0] != v[0], u[1] != v[1])

def mkchange(name):
    a, r = Ints(f'{name}_add {name}_rem')
    return (a, r), [a >= 0, r >= 0]

(xa, xr_dom) = mkchange('x'); x, xdom = (xa, xr_dom)
y, ydom = mkchange('y')
a, adom = mkchange('a')
b, bdom = mkchange('b')
c, cdom = mkchange('c')

# COMMUTATIVITY of the shipped max-union combine HOLDS: refute a disagreement.
s = Solver(); s.add(xdom + ydom); s.add(neq(cmax(x, y), cmax(y, x)))
expect("max-union combine commutativity holds", s, "unsat")

# NON-ASSOCIATIVITY of the shipped max-union combine (Finding A): find a triple
# where (a.b).c != a.(b.c). Expect SAT with a witness (e.g. add(x), add(x), rem(x)).
s = Solver(); s.add(adom + bdom + cdom)
s.add(neq(cmax(cmax(a, b), c), cmax(a, cmax(b, c))))
expect("max-union combine NON-associative (Finding A)", s, "sat")

# The concrete disclosed witness a=add, b=add, c=rem: (a.b).c = {} but a.(b.c) = {x}.
s = Solver()
s.add(a[0] == 1, a[1] == 0, b[0] == 1, b[1] == 0, c[0] == 0, c[1] == 1)
s.add(neq(cmax(cmax(a, b), c), cmax(a, cmax(b, c))))
expect("max-union: concrete add/add/rem witness differs by association", s, "sat")

# ASSOCIATIVITY of the sum-union FIX: refute a disagreement -> unsat (holds).
s = Solver(); s.add(adom + bdom + cdom)
s.add(neq(csum(csum(a, b), c), csum(a, csum(b, c))))
expect("sum-union FIX associativity holds", s, "unsat")

# COMMUTATIVITY of the sum-union FIX (for completeness): holds.
s = Solver(); s.add(xdom + ydom); s.add(neq(csum(x, y), csum(y, x)))
expect("sum-union FIX commutativity holds", s, "unsat")

print("== Z3 channel-netting monoid cross-witness: ALL PASS =="
      if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
