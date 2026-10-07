#!/usr/bin/env sage
"""
ll_identity_search.sage — Phase 4.7 bounded exhaustive search over
the ILLE algebraic identities satisfied by the F1R3FLY `Sig` algebra.

Complements the Rocq theorems in
`formal/rocq/cost_accounted_rho/theories/LLIdentities.v` and the Rust
proptest suite in `rholang/tests/accounting/ll_algebra_spec.rs` with
bounded exhaustive enumeration plus optional randomized stress. Any
counterexample found here is a bug in either the Rocq proof, the Rust
substrate, or this Python reference implementation — they must agree.

Models each Sig variant as a Python class. The `from_sig` reflection
mirrors `rholang/src/rust/interpreter/accounting/mod.rs::SignatureChannel::from_sig`:
- Unit → []
- Hash(b) → [hash_id(b)]
- And/Plus/With/Lolly/Threshold → concat(reflect(args)) + sort
- Bang/WhyNot → reflect(inner)
"""

import argparse
import hashlib
import itertools
import json
import os
import random
import sys
import tempfile


# ---------------------------------------------------------------------
# Sig algebra (Python reference implementation)
# ---------------------------------------------------------------------

class Sig:
    __slots__ = ("tag", "args")

    def __init__(self, tag, *args):
        self.tag = tag
        self.args = args

    def __repr__(self):
        return f"{self.tag}({', '.join(repr(a) for a in self.args)})"

    def __eq__(self, other):
        return isinstance(other, Sig) and self.tag == other.tag and self.args == other.args

    def __hash__(self):
        return hash((self.tag, self.args))


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


def required_units(sig):
    if sig.tag == "Unit":
        return 0
    if sig.tag == "Hash":
        return 1
    if sig.tag in ("And", "With", "Lolly"):
        return required_units(sig.args[0]) + required_units(sig.args[1])
    if sig.tag == "Plus":
        return min(required_units(sig.args[0]), required_units(sig.args[1]))
    if sig.tag == "Bang":
        return required_units(sig.args[0])
    if sig.tag == "WhyNot":
        return 0
    if sig.tag == "Threshold":
        return sig.args[0]
    raise ValueError(f"unknown Sig tag: {sig.tag}")


def plus_required_units(sig, branch):
    assert sig.tag == "Plus"
    return required_units(sig.args[branch])


def consumed_atoms(sig, plus_branch=0):
    if sig.tag == "Unit":
        return []
    if sig.tag == "Hash":
        return [hash_id(sig.args[0])]
    if sig.tag in ("And", "With", "Lolly"):
        return consumed_atoms(sig.args[0], plus_branch) + consumed_atoms(sig.args[1], plus_branch)
    if sig.tag == "Plus":
        return consumed_atoms(sig.args[plus_branch], plus_branch)
    if sig.tag == "Bang":
        return consumed_atoms(sig.args[0], plus_branch)
    if sig.tag == "WhyNot":
        return []
    if sig.tag == "Threshold":
        atoms = []
        for member in sig.args[1]:
            atoms += consumed_atoms(member, plus_branch)
        return atoms
    raise ValueError(f"unknown Sig tag: {sig.tag}")


def consume_atom_once(target, atoms):
    atoms = list(atoms)
    try:
        idx = atoms.index(target)
    except ValueError:
        return None
    return atoms[:idx] + atoms[idx + 1:]


# ---------------------------------------------------------------------
# Random Sig generator
# ---------------------------------------------------------------------

ATOM_POOL = [bytes([0xA0 + i]) for i in range(4)]


def random_sig(rng, depth):
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


def enumerate_sigs(depth, atom_count=2, threshold_member_bound=2):
    atoms = [bytes([0xA0 + i]) for i in range(atom_count)]
    if depth <= 0:
        return tuple(sorted({Unit(), *(Hash(a) for a in atoms)}, key=repr))

    inner = enumerate_sigs(depth - 1, atom_count, threshold_member_bound)
    values = set(inner)
    for left, right in itertools.product(inner, repeat=2):
        values.add(And(left, right))
        values.add(Plus(left, right))
        values.add(With(left, right))
        values.add(Lolly(left, right))
    for sig in inner:
        values.add(Bang(sig))
        values.add(WhyNot(sig))
    for n in range(1, threshold_member_bound + 1):
        for members in itertools.product(inner, repeat=n):
            for k in range(1, n + 1):
                values.add(Threshold(k, members))
    return tuple(sorted(values, key=repr))


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


def record_result(name, expected_holds, total, failures, counterexamples):
    successes = total - failures
    return {
        "identity": name,
        "expected_holds": expected_holds,
        "cases": total,
        "successes": successes,
        "failures": failures,
        "pass_rate": successes / total if total > 0 else 0.0,
        "counterexamples": counterexamples,
    }


def check_cases(name, expected_holds, cases):
    total = 0
    failures = 0
    counterexamples = []
    for lhs, rhs in cases:
        total += 1
        holds = channel_eq(lhs, rhs)
        if holds != expected_holds:
            failures += 1
            if len(counterexamples) < 3:
                counterexamples.append({
                    "lhs": repr(lhs),
                    "rhs": repr(rhs),
                    "lhs_channel": reflect(lhs),
                    "rhs_channel": reflect(rhs),
                })
    return record_result(name, expected_holds, total, failures, counterexamples)


def exhaustive_search(depth=1, atom_count=2, high_arity_depth=0, threshold_member_bound=2):
    domain = enumerate_sigs(depth, atom_count, threshold_member_bound)
    high_domain = enumerate_sigs(high_arity_depth, atom_count, threshold_member_bound)

    results = [
        check_cases(
            "tensor_commutative",
            True,
            ((And(s, t), And(t, s)) for s, t in itertools.product(domain, repeat=2)),
        ),
        check_cases(
            "tensor_associative",
            True,
            ((And(And(s, t), r), And(s, And(t, r))) for s, t, r in itertools.product(domain, repeat=3)),
        ),
        check_cases(
            "tensor_left_unit",
            True,
            ((And(Unit(), s), s) for s in domain),
        ),
        check_cases(
            "tensor_right_unit",
            True,
            ((And(s, Unit()), s) for s in domain),
        ),
        check_cases(
            "plus_commutative",
            True,
            ((Plus(s, t), Plus(t, s)) for s, t in itertools.product(domain, repeat=2)),
        ),
        check_cases(
            "with_commutative",
            True,
            ((With(s, t), With(t, s)) for s, t in itertools.product(domain, repeat=2)),
        ),
        check_cases(
            "bang_idempotent",
            True,
            ((Bang(Bang(s)), Bang(s)) for s in domain),
        ),
        check_cases(
            "whynot_idempotent",
            True,
            ((WhyNot(WhyNot(s)), WhyNot(s)) for s in domain),
        ),
        check_cases(
            "bang_monoidal",
            True,
            ((Bang(And(s, t)), And(Bang(s), Bang(t))) for s, t in itertools.product(domain, repeat=2)),
        ),
        check_cases(
            "bang_unit",
            True,
            ((Bang(Unit()), Unit()) for _ in (0,)),
        ),
        check_cases(
            "lolly_curry",
            True,
            ((Lolly(And(s, t), r), Lolly(s, Lolly(t, r))) for s, t, r in itertools.product(domain, repeat=3)),
        ),
        check_cases(
            "threshold_permutation",
            True,
            (
                (Threshold(k, members), Threshold(k, tuple(reversed(members))))
                for n in range(2, threshold_member_bound + 1)
                for members in itertools.product(domain, repeat=n)
                for k in range(1, n + 1)
            ),
        ),
        check_cases(
            "tensor_associator_pentagon",
            True,
            (
                (And(And(And(a, b), c), d), And(a, And(b, And(c, d))))
                for a, b, c, d in itertools.product(high_domain, repeat=4)
            ),
        ),
        check_cases(
            "tensor_unitor_triangle",
            True,
            ((And(And(a, Unit()), b), And(a, And(Unit(), b))) for a, b in itertools.product(domain, repeat=2)),
        ),
        check_cases(
            "anti_contraction (must fail)",
            False,
            ((And(s, s), s) for s in domain if reflect(s)),
        ),
        check_cases(
            "anti_weakening (must fail)",
            False,
            ((And(s, t), s) for s, t in itertools.product(domain, repeat=2) if reflect(t)),
        ),
    ]
    return results, {
        "domain_size": len(domain),
        "domain_depth": depth,
        "high_arity_domain_size": len(high_domain),
        "high_arity_depth": high_arity_depth,
        "atom_count": atom_count,
        "threshold_member_bound": threshold_member_bound,
    }


def check_resource_cases(name, cases):
    total = 0
    failures = 0
    counterexamples = []
    for payload in cases:
        total += 1
        ok, detail = payload
        if not ok:
            failures += 1
            if len(counterexamples) < 3:
                counterexamples.append(detail)
    return record_result(name, True, total, failures, counterexamples)


def resource_search(depth=1, atom_count=2, threshold_member_bound=2):
    domain = enumerate_sigs(depth, atom_count, threshold_member_bound)
    atoms = [Hash(bytes([0xA0 + i])) for i in range(atom_count)]

    results = [
        check_resource_cases(
            "resource_tensor_required_additive",
            (
                (
                    required_units(And(s, t)) == required_units(s) + required_units(t),
                    {"s": repr(s), "t": repr(t), "lhs": required_units(And(s, t)), "rhs": required_units(s) + required_units(t)},
                )
                for s, t in itertools.product(domain, repeat=2)
            ),
        ),
        check_resource_cases(
            "resource_with_requires_both_branches",
            (
                (
                    required_units(With(s, t)) == required_units(s) + required_units(t),
                    {"s": repr(s), "t": repr(t), "lhs": required_units(With(s, t)), "rhs": required_units(s) + required_units(t)},
                )
                for s, t in itertools.product(domain, repeat=2)
            ),
        ),
        check_resource_cases(
            "resource_plus_branch_required_units",
            (
                (
                    plus_required_units(Plus(s, t), branch) == required_units((s, t)[branch]),
                    {"s": repr(s), "t": repr(t), "branch": branch},
                )
                for s, t in itertools.product(domain, repeat=2)
                for branch in (0, 1)
            ),
        ),
        check_resource_cases(
            "resource_lolly_conservative",
            (
                (
                    required_units(Lolly(s, t)) == required_units(s) + required_units(t),
                    {"s": repr(s), "t": repr(t), "lhs": required_units(Lolly(s, t)), "rhs": required_units(s) + required_units(t)},
                )
                for s, t in itertools.product(domain, repeat=2)
            ),
        ),
        check_resource_cases(
            "resource_bang_reuses_inner_requirement",
            (
                (
                    required_units(Bang(s)) == required_units(s),
                    {"s": repr(s), "lhs": required_units(Bang(s)), "rhs": required_units(s)},
                )
                for s in domain
            ),
        ),
        check_resource_cases(
            "resource_whynot_requires_zero",
            (
                (
                    required_units(WhyNot(s)) == 0 and consumed_atoms(WhyNot(s)) == [],
                    {"s": repr(s), "required": required_units(WhyNot(s)), "consumed": consumed_atoms(WhyNot(s))},
                )
                for s in domain
            ),
        ),
        check_resource_cases(
            "resource_threshold_required_is_k",
            (
                (
                    required_units(Threshold(k, members)) == k and 1 <= k <= len(members),
                    {"k": k, "members": [repr(m) for m in members], "required": required_units(Threshold(k, members))},
                )
                for n in range(1, threshold_member_bound + 1)
                for members in itertools.product(domain, repeat=n)
                for k in range(1, n + 1)
            ),
        ),
        check_resource_cases(
            "resource_nonbang_contraction_increases_required_units",
            (
                (
                    required_units(And(s, s)) > required_units(s),
                    {"s": repr(s), "single": required_units(s), "doubled": required_units(And(s, s))},
                )
                for s in domain
                if required_units(s) > 0
            ),
        ),
        check_resource_cases(
            "resource_nonwhynot_weakening_increases_required_units",
            (
                (
                    required_units(And(s, t)) > required_units(s),
                    {"s": repr(s), "t": repr(t), "base": required_units(s), "weakened": required_units(And(s, t))},
                )
                for s, t in itertools.product(domain, repeat=2)
                if required_units(t) > 0
            ),
        ),
        check_resource_cases(
            "resource_single_witness_no_double_spend",
            (
                (
                    consume_atom_once(reflect(atom)[0], consume_atom_once(reflect(atom)[0], reflect(atom)) or []) is None,
                    {"atom": repr(atom), "channel": reflect(atom)},
                )
                for atom in atoms
            ),
        ),
        check_resource_cases(
            "resource_duplicate_witness_allows_two_spends",
            (
                (
                    consume_atom_once(
                        reflect(atom)[0],
                        consume_atom_once(reflect(atom)[0], reflect(atom) + reflect(atom)) or [],
                    ) == [],
                    {"atom": repr(atom), "channel": reflect(atom) + reflect(atom)},
                )
                for atom in atoms
            ),
        ),
    ]
    return results, {
        "domain_size": len(domain),
        "domain_depth": depth,
        "atom_count": atom_count,
        "threshold_member_bound": threshold_member_bound,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--mode", choices=("exhaustive", "random", "both"), default="exhaustive")
    parser.add_argument("--samples", type=int, default=10_000)
    parser.add_argument("--depth", type=int, default=4)
    parser.add_argument("--exhaustive-depth", type=int, default=1)
    parser.add_argument("--high-arity-depth", type=int, default=0)
    parser.add_argument("--atoms", type=int, default=2)
    parser.add_argument("--threshold-member-bound", type=int, default=2)
    parser.add_argument("--seed", type=lambda s: int(s, 0), default=0xCAFEF00D)
    parser.add_argument(
        "--output",
        default=os.path.join(tempfile.gettempdir(), "ll_identity_search_results.json"),
    )
    args = parser.parse_args()

    sections = {}
    results = []
    if args.mode in ("exhaustive", "both"):
        exhaustive_results, exhaustive_meta = exhaustive_search(
            depth=args.exhaustive_depth,
            atom_count=args.atoms,
            high_arity_depth=args.high_arity_depth,
            threshold_member_bound=args.threshold_member_bound,
        )
        resource_results, resource_meta = resource_search(
            depth=args.exhaustive_depth,
            atom_count=args.atoms,
            threshold_member_bound=args.threshold_member_bound,
        )
        sections["exhaustive"] = {
            "metadata": exhaustive_meta,
            "identities": exhaustive_results,
        }
        sections["resources"] = {
            "metadata": resource_meta,
            "obligations": resource_results,
        }
        results.extend(exhaustive_results)
        results.extend(resource_results)

    if args.mode in ("random", "both"):
        random_results = search(samples=args.samples, depth=args.depth, seed=args.seed)
        sections["random"] = {
            "samples_per_identity": args.samples,
            "depth": args.depth,
            "seed": hex(args.seed),
            "identities": random_results,
        }
        results.extend(random_results)

    overall_pass = all(r["failures"] == 0 for r in results)
    output = {
        "mode": args.mode,
        "overall_pass": overall_pass,
        "sections": sections,
    }
    with open(args.output, "w") as f:
        json.dump(output, f, indent=2, default=str)

    print(f"Phase 4.7 LL identity bounded verification")
    print(f"  mode: {args.mode}")
    print(f"  results: {args.output}")
    print()
    width = max(len(r["identity"]) for r in results)
    for r in results:
        marker = "PASS" if r["failures"] == 0 else "FAIL"
        total = r.get("samples", r.get("cases", 0))
        print(
            f"  [{marker}] {r['identity']:<{width}}  "
            f"successes={r['successes']}/{total}  "
            f"failures={r['failures']}"
        )
    print()
    if overall_pass:
        print("ALL REQUESTED BOUNDS VERIFIED — no counterexamples found.")
        sys.exit(0)
    else:
        print("COUNTEREXAMPLES FOUND — see JSON output.")
        sys.exit(1)


if __name__ == "__main__":
    main()
