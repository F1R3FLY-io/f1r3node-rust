#!/usr/bin/env python3
"""Z3 cross-witness: the algebra of the six VectorDB metrics (P6, decision 54).

The kernels of rholang/src/rust/interpreter/tensor/exact.rs compute exact values:

  * dot(a, b) = sum a_i b_i, dist2(a, b) = sum (a_i - b_i)^2, l1(a, b) = sum |a_i - b_i|
    over three-dimensional real vectors (the reals contain the exact rationals);
  * hamming(a, b) = popcount(a xor b), and the jaccard counts popcount(a and b) and
    popcount(a or b) over 8-bit binary vectors.

The script checks the laws that Rocq proves in Similarity.v (S6, S7 and S12) and the
polarization identity dist2(a, b) = |a|^2 - 2 a.b + |b|^2, which links euclidean to
dot. Each law is checked by refuting its negation ("unsat"). The negative controls
show two laws that fail: the triangle inequality of dist2, and the zero law of dot.
Exit status 0 when every check has its expected result, 1 otherwise.
"""
import sys

from z3 import BitVecs, Extract, If, Or, Reals, Solver, Sum, ZeroExt

results = []


def expect(name, constraints, want):
    solver = Solver()
    solver.add(*constraints)
    got = str(solver.check())
    good = got == want
    results.append(good)
    print(f"  {'PASS' if good else 'FAIL'}  {name}: {got} (expected {want})")
    if good and want == "sat":
        print(f"        witness: {solver.model()}")


def absolute(x):
    return If(x >= 0, x, -x)


def dot(a, b):
    return Sum([x * y for x, y in zip(a, b)])


def dist2(a, b):
    return Sum([(x - y) * (x - y) for x, y in zip(a, b)])


def l1(a, b):
    return Sum([absolute(x - y) for x, y in zip(a, b)])


a = Reals("a0 a1 a2")
b = Reals("b0 b1 b2")
c = Reals("c0 c1 c2")

print("== Z3: the algebra of the VectorDB metrics")
expect("dot is symmetric", [dot(a, b) != dot(b, a)], "unsat")
expect("dot is invariant under a permutation of the coordinates",
       [dot(a, b) != dot([a[2], a[0], a[1]], [b[2], b[0], b[1]])], "unsat")
expect("polarization: dist2(a, b) = |a|^2 - 2 a.b + |b|^2",
       [dist2(a, b) != dot(a, a) - 2 * dot(a, b) + dot(b, b)], "unsat")
expect("dist2 is non-negative", [dist2(a, b) < 0], "unsat")
expect("dist2 is symmetric", [dist2(a, b) != dist2(b, a)], "unsat")
expect("dist2 is zero only on equal vectors",
       [dist2(a, b) == 0, Or([x != y for x, y in zip(a, b)])], "unsat")
expect("l1 is non-negative", [l1(a, b) < 0], "unsat")
expect("l1 is symmetric", [l1(a, b) != l1(b, a)], "unsat")
expect("l1 satisfies the triangle inequality", [l1(a, c) > l1(a, b) + l1(b, c)], "unsat")


def popcount(v):
    return Sum([ZeroExt(7, Extract(i, i, v)) for i in range(8)])


p, q, r = BitVecs("p q r", 8)
hamming = lambda x, y: popcount(x ^ y)
expect("hamming is symmetric", [hamming(p, q) != hamming(q, p)], "unsat")
expect("hamming satisfies the triangle inequality",
       [hamming(p, r) > hamming(p, q) + hamming(q, r)], "unsat")
expect("hamming is zero only on equal vectors", [hamming(p, q) == 0, p != q], "unsat")
expect("jaccard: inter <= union", [popcount(p & q) > popcount(p | q)], "unsat")
expect("jaccard: the counts are symmetric",
       [Or(popcount(p & q) != popcount(q & p), popcount(p | q) != popcount(q | p))], "unsat")
expect("jaccard: the union is empty only when neither vector sets a bit",
       [popcount(p | q) == 0, Or(p != 0, q != 0)], "unsat")

print("-- negative controls")
expect("control: dist2 breaks the triangle inequality (euclidean compares squares)",
       [dist2(a, c) > dist2(a, b) + dist2(b, c)], "sat")
expect("control: dot is zero on vectors that differ (dot is not a distance)",
       [dot(a, b) == 0, Or([x != y for x, y in zip(a, b)])], "sat")

ok = all(results)
print("== Z3 algebra of the VectorDB metrics: ALL PASS" if ok else "== FAILURES ==")
sys.exit(0 if ok else 1)
