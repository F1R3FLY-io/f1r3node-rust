#!/usr/bin/env python3
"""Z3 cross-witness: the squared forms of the VectorDB decisions (P6, decision 54).

The VectorDB family decides every selection with exact rationals and never takes a
square root (rholang/src/rust/interpreter/tensor/exact.rs). For a query q and a record
e, let u = q.e, N = |q|^2 |e|^2 > 0 and s = |q| |e| = sqrt(N) > 0. The real cosine is
cos = u / s, so cos >= t holds exactly when t*s <= u. The Rust function `within`
decides it without s:

    cosine_within(u, t, N) :=
         (0 <= u and t < 0)
      or (0 <= u and 0 <= t and t^2 N <= u^2)
      or (u < 0 and t < 0 and u^2 <= t^2 N)

Z3 decides these sentences over the real numbers with its complete procedure for
nonlinear real arithmetic (nlsat). So this script checks the claims in the real
field itself, where the root s exists. Rocq proves the same claims over every ordered
commutative ring (formal/rocq/reified_rspaces/theories/Similarity.v, S1-S3, S8, S9).

Each claim is checked by refuting its negation ("unsat"). Each negative control
reverts one rule and must have a counterexample ("sat"):
  * squares only: cosine compares t^2 N with u^2 and ignores the signs;
  * swapped norms: the cosine order multiplies u_i^2 by its own norm;
  * radius unsquared: euclidean compares d^2 with r instead of r^2.
Exit status 0 when every check has its expected result, 1 otherwise.
"""
import sys

from z3 import And, Not, Or, Reals, Solver

results = []


def expect(name, constraints, want):
    """Check `constraints` and compare the result with `want` ("sat" or "unsat")."""
    solver = Solver()
    solver.add(*constraints)
    got = str(solver.check())
    good = got == want
    results.append(good)
    print(f"  {'PASS' if good else 'FAIL'}  {name}: {got} (expected {want})")
    if good and want == "sat":
        print(f"        witness: {solver.model()}")


def cosine_within(u, t, n):
    return Or(And(0 <= u, t < 0),
              And(0 <= u, 0 <= t, t * t * n <= u * u),
              And(u < 0, t < 0, u * u <= t * t * n))


def cosine_le(u1, n1, u2, n2):
    """cos_1 <= cos_2 for cos_i = u_i / sqrt(nq n_i): the order of `better`."""
    return Or(And(u1 < 0, 0 <= u2),
              And(0 <= u1, 0 <= u2, u1 * u1 * n2 <= u2 * u2 * n1),
              And(u1 < 0, u2 < 0, u2 * u2 * n1 <= u1 * u1 * n2))


def euclidean_within(d2, r):
    return And(0 <= r, d2 <= r * r)


u, t, s = Reals("u t s")
u1, u2, s1, s2, nq, n1, n2 = Reals("u1 u2 s1 s2 nq n1 n2")
d, r = Reals("d r")

print("== Z3: the squared forms of the VectorDB decisions")

# S1: t*s <= u (cos >= t) exactly when cosine_within(u, t, s^2).
expect("cosine threshold: t*s <= u iff cosine_within(u, t, s*s)",
       [s > 0, Not((t * s <= u) == cosine_within(u, t, s * s))], "unsat")

# S2: u1*s2 <= u2*s1 (cos_1 <= cos_2) exactly when cosine_le(u1, n1, u2, n2).
order_premises = [nq > 0, s1 > 0, s2 > 0, s1 * s1 == nq * n1, s2 * s2 == nq * n2]
expect("cosine order: u1*s2 <= u2*s1 iff cosine_le(u1, n1, u2, n2)",
       order_premises + [Not((u1 * s2 <= u2 * s1) == cosine_le(u1, n1, u2, n2))], "unsat")

# S3: a distance d >= 0 is within r exactly when euclidean_within(d^2, r).
expect("euclidean radius: d <= r iff euclidean_within(d*d, r)",
       [d >= 0, Not((d <= r) == euclidean_within(d * d, r))], "unsat")

# S4: a lower cosine threshold admits every record that a higher one admits.
t1, t2, nn = Reals("t1 t2 nn")
expect("cosine threshold monotone",
       [nn > 0, t1 <= t2, cosine_within(u, t2, nn), Not(cosine_within(u, t1, nn))], "unsat")

# S5: a larger radius admits every record that a smaller one admits.
r1, r2, dd = Reals("r1 r2 dd")
expect("euclidean radius monotone",
       [r1 <= r2, euclidean_within(dd, r1), Not(euclidean_within(dd, r2))], "unsat")

# S8 and S9 in three dimensions: Cauchy-Schwarz, every cosine passes -1, and no
# cosine passes a threshold above 1.
a0, a1, a2, b0, b1, b2 = Reals("a0 a1 a2 b0 b1 b2")
ab = a0 * b0 + a1 * b1 + a2 * b2
aa = a0 * a0 + a1 * a1 + a2 * a2
bb = b0 * b0 + b1 * b1 + b2 * b2
expect("Cauchy-Schwarz in three dimensions", [Not(ab * ab <= aa * bb)], "unsat")
expect("every cosine passes the threshold -1",
       [aa * bb > 0, Not(cosine_within(ab, -1, aa * bb))], "unsat")
expect("no cosine passes a threshold above 1",
       [aa * bb > 0, t > 1, cosine_within(ab, t, aa * bb)], "unsat")

# The cosine threshold 1 admits exactly the records parallel to the query.
expect("the threshold 1 admits parallel records (witness)",
       [aa * bb > 0, cosine_within(ab, 1, aa * bb)], "sat")

print("-- negative controls (each reverts one rule and must have a counterexample)")


def squares_only(u, t, n):
    return t * t * n <= u * u


expect("control: squares only differs from cos >= t",
       [s > 0, Not((t * s <= u) == squares_only(u, t, s * s))], "sat")


def cosine_le_swapped(u1, n1, u2, n2):
    return Or(And(u1 < 0, 0 <= u2),
              And(0 <= u1, 0 <= u2, u1 * u1 * n1 <= u2 * u2 * n2),
              And(u1 < 0, u2 < 0, u2 * u2 * n2 <= u1 * u1 * n1))


expect("control: swapped norms differ from the cosine order",
       order_premises + [Not((u1 * s2 <= u2 * s1) == cosine_le_swapped(u1, n1, u2, n2))], "sat")


def euclidean_unsquared(d2, r):
    return And(0 <= r, d2 <= r)


expect("control: an unsquared radius differs from d <= r",
       [d >= 0, Not((d <= r) == euclidean_unsquared(d * d, r))], "sat")

ok = all(results)
print("== Z3 squared forms of the VectorDB decisions: ALL PASS" if ok else "== FAILURES ==")
sys.exit(0 if ok else 1)
