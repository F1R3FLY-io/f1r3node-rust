#!/usr/bin/env python3
"""Z3 cross-witness: the selection order of the VectorDB family (decisions 41 and 60).

A VectorDB receive orders its eligible records by the better score first, then by
the lower kappa (the DR-75 key of decision 41: the hash of the source produce, then
the full digest), then by the lower index in the view (design v3, section 4.4). Single
mode takes the first record that binds, and list mode takes the first k records. So
every node must compute one order, whatever the order in which it reads the view.

A record is (score, kappa, index), and "a precedes b" is

    score_a > score_b  or  (score_a = score_b and (kappa_a, index_a) <lex (kappa_b, index_b)).

On records with distinct (kappa, index) this is a strict total order, so the sorted
arrangement of a set of records is unique and top-k is deterministic (Rocq
Similarity.v, S13 and S14). The scores of cosine and jaccard compare by
cross-multiplication, a1*b2 <= a2*b1 with b1, b2 > 0; the script checks that this
order is transitive and total (S10).

The negative control drops the tie-break: by score alone, two distinct records with
equal scores are incomparable, so the first record is not determined (S16).
Exit status 0 when every check has its expected result, 1 otherwise.
"""
import sys

from z3 import And, Ints, Not, Or, Reals, Solver

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


def precedes(a, b):
    (sa, ka, ia), (sb, kb, ib) = a, b
    return Or(sa > sb, And(sa == sb, Or(ka < kb, And(ka == kb, ia < ib))))


def distinct_keys(a, b):
    return Or(a[1] != b[1], a[2] != b[2])


sa, sb, sc = Reals("sa sb sc")
ka, kb, kc, ia, ib, ic = Ints("ka kb kc ia ib ic")
a, b, c = (sa, ka, ia), (sb, kb, ib), (sc, kc, ic)

print("== Z3: the selection order of the VectorDB family")
expect("irreflexive", [precedes(a, a)], "unsat")
expect("asymmetric", [precedes(a, b), precedes(b, a)], "unsat")
expect("transitive", [precedes(a, b), precedes(b, c), Not(precedes(a, c))], "unsat")
expect("total on distinct tie keys",
       [distinct_keys(a, b), Not(precedes(a, b)), Not(precedes(b, a))], "unsat")
expect("the first record is unique on distinct tie keys",
       [distinct_keys(a, b), Not(precedes(b, a)), Not(precedes(a, b))], "unsat")

# The cross-multiplied order of cosine (a = ssq(q.e), b = |e|^2) and jaccard
# (a = the intersection, b = the union): a1/b1 <= a2/b2 as a1*b2 <= a2*b1.
x1, x2, x3, y1, y2, y3 = Reals("x1 x2 x3 y1 y2 y3")


def ratio_le(xa, ya, xb, yb):
    return xa * yb <= xb * ya


positive = [y1 > 0, y2 > 0, y3 > 0]
expect("cross-multiplied order is transitive",
       positive + [ratio_le(x1, y1, x2, y2), ratio_le(x2, y2, x3, y3),
                   Not(ratio_le(x1, y1, x3, y3))], "unsat")
expect("cross-multiplied order is total",
       positive + [Not(ratio_le(x1, y1, x2, y2)), Not(ratio_le(x2, y2, x1, y1))], "unsat")
expect("cross-multiplied order equals the order of the quotients",
       positive + [Not(ratio_le(x1, y1, x2, y2) == (x1 / y1 <= x2 / y2))], "unsat")

print("-- negative controls")


def precedes_by_score(a, b):
    return a[0] > b[0]


expect("control: by score alone, two distinct records can be incomparable",
       [distinct_keys(a, b), Not(precedes_by_score(a, b)), Not(precedes_by_score(b, a))], "sat")
expect("control: without a positive union, the cross-multiplied order is not transitive",
       [ratio_le(x1, y1, x2, y2), ratio_le(x2, y2, x3, y3), Not(ratio_le(x1, y1, x3, y3))], "sat")

ok = all(results)
print("== Z3 selection order of the VectorDB family: ALL PASS" if ok else "== FAILURES ==")
sys.exit(0 if ok else 1)
