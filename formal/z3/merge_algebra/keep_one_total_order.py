#!/usr/bin/env python3
# Z3 cross-witness for the P3 / no-fork determinism linchpin: four priority
# tiers followed by an injective composite identity key.
# DeployChainIndex comparator (casper/src/rust/merging/deploy_chain_index.rs
# :151-230) is a STRICT TOTAL ORDER, so the merge's `min_by`/`sort` winner is
# UNIQUE and independent of the (reseeded) HashSet iteration order upstream --
# i.e. NO cross-node fork. The abstract tower is:
#     1. Sigma-cost         DESCENDING
#     2. max single cost    DESCENDING
#     3. min deploy_id       ASCENDING
#     4. post_state_hash     ASCENDING
#     5. composite identity  ASCENDING   (the INJECTIVE Eq/Hash key)
# Because keys 1-4 are FUNCTIONS of the deploy set, and key 5 IS the set (the Eq
# key), `cmp a b == Equal` iff a == b -- no two DISTINCT chains ever tie. This
# confirms Rocq KeepOneOrder.keep_one_total_order / keep_one_equal_impl_eq /
# output_indep_of_input_perm (all UNCONDITIONAL -- no NoDup premise, unlike the
# fork-choice TieBreak analogue). The `sat` probe at the end is the DUAL
# counterexample: WITHOUT key 5, two DISTINCT chains sharing keys 1-4 tie, which
# is exactly the non-determinism the 5th key eliminates.
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

# entry = (k1 Sigma-cost, k2 max-cost, k3 min-id, k4 hash, k5 injective set-key).
# "a precedes b" in the merge ranking: keys 1-2 DESCENDING, keys 3-5 ASCENDING.
def prec5(a, b):
    k1a, k2a, k3a, k4a, k5a = a
    k1b, k2b, k3b, k4b, k5b = b
    return Or(k1a > k1b,
           And(k1a == k1b, Or(k2a > k2b,
           And(k2a == k2b, Or(k3a < k3b,
           And(k3a == k3b, Or(k4a < k4b,
           And(k4a == k4b, k5a < k5b))))))))

# the buggy 4-key comparator (pre-fix): keys 1-4 only, no injective terminal key.
def prec4(a, b):
    k1a, k2a, k3a, k4a, _ = a
    k1b, k2b, k3b, k4b, _ = b
    return Or(k1a > k1b,
           And(k1a == k1b, Or(k2a > k2b,
           And(k2a == k2b, Or(k3a < k3b,
           And(k3a == k3b, k4a < k4b))))))

def mk(name):
    return tuple(Ints(' '.join(f'{name}{i}' for i in range(1, 6))))

a, b, c = mk('a'), mk('b'), mk('c')

# irreflexive: nothing precedes itself
s = Solver(); s.add(prec5(a, a))
expect("irreflexive", s, "unsat")

# asymmetric: prec5(a,b) and prec5(b,a) cannot both hold
s = Solver(); s.add(prec5(a, b), prec5(b, a))
expect("asymmetric", s, "unsat")

# transitive: prec5(a,b) and prec5(b,c) implies prec5(a,c) -- refute the negation
s = Solver(); s.add(prec5(a, b), prec5(b, c), Not(prec5(a, c)))
expect("transitive", s, "unsat")

# total on DISTINCT injective keys: k5a != k5b implies prec5(a,b) or prec5(b,a).
s = Solver(); s.add(a[4] != b[4], Not(prec5(a, b)), Not(prec5(b, a)))
expect("total on distinct injective key", s, "unsat")

# argmax uniqueness: two entries with DISTINCT injective keys cannot BOTH be
# maximal (an entry is maximal iff nothing precedes it). By totality one must
# precede the other, so the merge winner is unique -- no HashSet-order tie, no fork.
s = Solver(); s.add(a[4] != b[4], Not(prec5(b, a)), Not(prec5(a, b)))
expect("argmax unique on distinct injective key", s, "unsat")

# WHY THE 5TH KEY IS NEEDED (dual): without the injective terminal key, two
# DISTINCT chains (k5a != k5b) that agree on keys 1-4 TIE under prec4 -- both
# `prec4` false -- so `min_by`/`sort` would decide by HashSet order (a fork).
s = Solver()
s.add(a[4] != b[4],                                  # genuinely distinct chains
      a[0] == b[0], a[1] == b[1], a[2] == b[2], a[3] == b[3],  # equal keys 1-4
      Not(prec4(a, b)), Not(prec4(b, a)))            # ...tie under the 4-key order
expect("WITHOUT key 5, distinct chains tie (fork)", s, "sat")

print("== Z3 keep-one total-order cross-witness: ALL PASS =="
      if ok else "== FAILURES ==")
import sys
sys.exit(0 if ok else 1)
