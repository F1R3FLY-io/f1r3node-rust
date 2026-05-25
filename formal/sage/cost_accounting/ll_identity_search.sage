#!/usr/bin/env sage
"""
ll_identity_search.sage — Phase 4.7 randomized exhaustive search over
the ILLE algebraic identities satisfied by the F1R3FLY `Sig` algebra.

Complements the Rocq theorems in
`formal/rocq/cost_accounted_rho/theories/LLIdentities.v` and the Rust
proptest suite in `rholang/tests/accounting/ll_algebra_spec.rs` with a
finite-state randomized search. Any counterexample found here is a bug
in either the Rocq proof, the Rust substrate, or this Python
reference implementation — they must agree.

Models each Sig variant as a Python class. The `from_sig` reflection
mirrors `rholang/src/rust/interpreter/accounting/mod.rs::SignatureChannel::from_sig`:
- Unit → []
- Hash(b) → [hash_id(b)]
- And/Plus/With/Lolly/Threshold → concat(reflect(args)) + sort
- Bang/WhyNot → reflect(inner)
"""

import argparse
import hashlib
import json
import os
import random
import sys


# ---------------------------------------------------------------------
# Sig algebra (Python reference implementation)
# ---------------------------------------------------------------------

class Sig:
    """Tag-based discriminated-union — mirrors Rust enum variants."""
    __slots__ = ("tag", "args")

    def __init__(self, tag, *args):
        self.tag = tag
        self.args = args

    def __repr__(self):
        return f"{self.tag}({', '.join(repr(a) for a in self.args)})"


def Unit():
    return Sig("Unit")


def Hash(bytes_):
    return Sig("Hash", bytes(bytes_))


def And(left, right):
    return Sig("And", left, right)


def Plus(left, right):
    return Sig("Plus", left, right)


def With(left, right):
    return Sig("With", left, right)


def Bang(inner):
    return Sig("Bang", inner)


def WhyNot(inner):
    return Sig("WhyNot", inner)


def Lolly(from_, to):
    return Sig("Lolly", from_, to)


def Threshold(k, members):
    return Sig("Threshold", k, tuple(members))


# ---------------------------------------------------------------------
# Channel reflection (Python mirror of from_sig)
# ---------------------------------------------------------------------

def hash_id(bs):
    """Stable Blake2b-style atom-id derived from the byte payload."""
    return hashlib.blake2b(bs, digest_size=16).hexdigest()


def reflect(sig):
    """Return the canonical channel as a sorted list of atom ids."""
    if sig.tag == "Unit":
        return []
    if sig.tag == "Hash":
        return [hash_id(sig.args[0])]
    if sig.tag in ("And", "Plus", "With", "Lolly"):
        return sorted(reflect(sig.args[0]) + reflect(sig.args[1]))
    if sig.tag in ("Bang", "WhyNot"):
        return reflect(sig.args[0])
    if sig.tag == "Threshold":
        chans = []
        for m in sig.args[1]:
            chans += reflect(m)
        return sorted(chans)
    raise ValueError(f"unknown Sig tag: {sig.tag}")


def channel_eq(a, b):
    return reflect(a) == reflect(b)


# ---------------------------------------------------------------------
# Random Sig generator
# ---------------------------------------------------------------------

ATOM_POOL = [bytes([0xA0 + i]) for i in range(4)]


def random_sig(rng, depth):
    """Generate a random Sig with bounded recursion depth."""
    if depth <= 0:
        return rng.choice([Unit(), Hash(rng.choice(ATOM_POOL))])
    kind = rng.randint(0, 10)
    if kind == 0:
        return Unit()
    if kind == 1:
        return Hash(rng.choice(ATOM_POOL))
    if kind == 2:
        return And(random_sig(rng, depth - 1), random_sig(rng, depth - 1))
    if kind == 3:
        return Plus(random_sig(rng, depth - 1), random_sig(rng, depth - 1))
    if kind == 4:
        return With(random_sig(rng, depth - 1), random_sig(rng, depth - 1))
    if kind == 5:
        return Bang(random_sig(rng, depth - 1))
    if kind == 6:
        return WhyNot(random_sig(rng, depth - 1))
    if kind == 7:
        return Lolly(random_sig(rng, depth - 1), random_sig(rng, depth - 1))
    if kind == 8:
        n = rng.randint(1, 3)
        k = rng.randint(1, n)
        return Threshold(k, [random_sig(rng, depth - 1) for _ in range(n)])
    if kind == 9:
        return Unit()
    return Hash(rng.choice(ATOM_POOL))


# ---------------------------------------------------------------------
# Identity catalog — pairs (name, builder)
#   builder: rng -> (lhs, rhs) for channel_eq(lhs, rhs)
# Also includes anti-identities expected to FAIL — checked with negated assertion.
# ---------------------------------------------------------------------

def id_tensor_commutative(rng, depth):
    s = random_sig(rng, depth)
    t = random_sig(rng, depth)
    return And(s, t), And(t, s)


def id_tensor_associative(rng, depth):
    s = random_sig(rng, depth)
    t = random_sig(rng, depth)
    r = random_sig(rng, depth)
    return And(And(s, t), r), And(s, And(t, r))


def id_tensor_left_unit(rng, depth):
    s = random_sig(rng, depth)
    return And(Unit(), s), s


def id_tensor_right_unit(rng, depth):
    s = random_sig(rng, depth)
    return And(s, Unit()), s


def id_plus_commutative(rng, depth):
    s, t = random_sig(rng, depth), random_sig(rng, depth)
    return Plus(s, t), Plus(t, s)


def id_with_commutative(rng, depth):
    s, t = random_sig(rng, depth), random_sig(rng, depth)
    return With(s, t), With(t, s)


def id_bang_idempotent(rng, depth):
    s = random_sig(rng, depth)
    return Bang(Bang(s)), Bang(s)


def id_whynot_idempotent(rng, depth):
    s = random_sig(rng, depth)
    return WhyNot(WhyNot(s)), WhyNot(s)


def id_bang_monoidal(rng, depth):
    s, t = random_sig(rng, depth), random_sig(rng, depth)
    return Bang(And(s, t)), And(Bang(s), Bang(t))


def id_bang_unit(rng, depth):
    return Bang(Unit()), Unit()


def id_lolly_curry(rng, depth):
    s, t, r = (
        random_sig(rng, depth),
        random_sig(rng, depth),
        random_sig(rng, depth),
    )
    return Lolly(And(s, t), r), Lolly(s, Lolly(t, r))


def id_threshold_permutation(rng, depth):
    n = rng.randint(2, 4)
    members = [random_sig(rng, depth) for _ in range(n)]
    k = rng.randint(1, n)
    return Threshold(k, members), Threshold(k, list(reversed(members)))


def id_pentagon(rng, depth):
    a, b, c, d = (random_sig(rng, depth) for _ in range(4))
    return (
        And(And(And(a, b), c), d),
        And(a, And(b, And(c, d))),
    )


def id_triangle(rng, depth):
    a, b = random_sig(rng, depth), random_sig(rng, depth)
    return And(And(a, Unit()), b), And(a, And(Unit(), b))


def anti_id_contraction(rng, depth):
    """Should FAIL for non-trivial-channel σ: `σ ⊗ σ ≢ σ`."""
    while True:
        s = random_sig(rng, depth)
        if reflect(s):
            break
    return And(s, s), s


def anti_id_weakening(rng, depth):
    """Should FAIL for non-trivial τ: `σ ⊗ τ ≢ σ`."""
    while True:
        s = random_sig(rng, depth)
        t = random_sig(rng, depth)
        if reflect(t):
            break
    return And(s, t), s


IDENTITIES = [
    ("tensor_commutative", id_tensor_commutative, True),
    ("tensor_associative", id_tensor_associative, True),
    ("tensor_left_unit", id_tensor_left_unit, True),
    ("tensor_right_unit", id_tensor_right_unit, True),
    ("plus_commutative", id_plus_commutative, True),
    ("with_commutative", id_with_commutative, True),
    ("bang_idempotent", id_bang_idempotent, True),
    ("whynot_idempotent", id_whynot_idempotent, True),
    ("bang_monoidal", id_bang_monoidal, True),
    ("bang_unit", id_bang_unit, True),
    ("lolly_curry", id_lolly_curry, True),
    ("threshold_permutation", id_threshold_permutation, True),
    ("tensor_associator_pentagon", id_pentagon, True),
    ("tensor_unitor_triangle", id_triangle, True),
    ("anti_contraction (must fail)", anti_id_contraction, False),
    ("anti_weakening (must fail)", anti_id_weakening, False),
]


# ---------------------------------------------------------------------
# Search driver
# ---------------------------------------------------------------------

def search(samples=10_000, depth=4, seed=0xCAFEF00D):
    rng = random.Random(seed)
    results = []
    for (name, builder, expected_holds) in IDENTITIES:
        successes = 0
        failures = 0
        counterexamples = []
        for _ in range(samples):
            lhs, rhs = builder(rng, depth)
            holds = channel_eq(lhs, rhs)
            if holds == expected_holds:
                successes += 1
            else:
                failures += 1
                if len(counterexamples) < 3:
                    counterexamples.append({
                        "lhs": repr(lhs),
                        "rhs": repr(rhs),
                        "lhs_channel": reflect(lhs),
                        "rhs_channel": reflect(rhs),
                    })
        results.append({
            "identity": name,
            "expected_holds": expected_holds,
            "samples": samples,
            "successes": successes,
            "failures": failures,
            "pass_rate": successes / samples if samples > 0 else 0.0,
            "counterexamples": counterexamples,
        })
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--samples", type=int, default=10_000)
    parser.add_argument("--depth", type=int, default=4)
    parser.add_argument("--seed", type=lambda s: int(s, 0), default=0xCAFEF00D)
    parser.add_argument(
        "--output",
        default=os.path.join(
            os.path.dirname(os.path.abspath(sys.argv[0])),
            "ll_identity_search_results.json",
        ),
    )
    args = parser.parse_args()

    results = search(samples=args.samples, depth=args.depth, seed=args.seed)

    overall_pass = all(r["failures"] == 0 for r in results)
    output = {
        "samples_per_identity": args.samples,
        "depth": args.depth,
        "seed": hex(args.seed),
        "overall_pass": overall_pass,
        "identities": results,
    }
    with open(args.output, "w") as f:
        json.dump(output, f, indent=2, default=str)

    print(f"Phase 4.7 LL identity exhaustive search")
    print(f"  samples per identity: {args.samples}")
    print(f"  depth: {args.depth}")
    print(f"  seed: {hex(args.seed)}")
    print(f"  results: {args.output}")
    print()
    width = max(len(r["identity"]) for r in results)
    for r in results:
        marker = "PASS" if r["failures"] == 0 else "FAIL"
        print(
            f"  [{marker}] {r['identity']:<{width}}  "
            f"successes={r['successes']}/{r['samples']}  "
            f"failures={r['failures']}"
        )
    print()
    if overall_pass:
        print("ALL IDENTITIES VERIFIED — no counterexamples found.")
        sys.exit(0)
    else:
        print("COUNTEREXAMPLES FOUND — see JSON output.")
        sys.exit(1)


if __name__ == "__main__":
    main()
