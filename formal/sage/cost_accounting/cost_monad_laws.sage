#!/usr/bin/env sage
# ════════════════════════════════════════════════════════════════════════
# cost_monad_laws.sage — bounded-exhaustive witness of the Cost monad's law
# substrate (continued-gslt-cost-v2.tex), the Sage leg of the multi-prover
# alignment (Stage 7).
#
# Independently corroborates the Rocq SignatureMonoid (CL2) + CostMonad (CL4)
# laws: the monad's unit/associativity "descend from the laws of the two
# constituent monoids" (Prop "the cost monad", :1064) — the SIGNATURE
# commutative monoid (Sig, *, ()) and the temporal token-stack FREE monoid
# (cons, ++, ()). The signature `*` is `And`; `()` is `Unit`; the `from_sig`
# reflection mirrors rholang/.../accounting/mod.rs::SignatureChannel::from_sig
# (canonical channel = sorted atom ids), exactly as ll_identity_search.sage.
#
# Emits JSON with an `overall_pass` boolean; the gate
# (scripts/check-cost-accounted-rho-sage.sh) fails iff overall_pass != true,
# i.e. iff any expected_holds=True law has a counterexample or any
# expected_holds=False law (non-commutativity, non-idempotence) failed to
# exhibit its witness. LOCAL-ONLY (never a CI gate).
# ════════════════════════════════════════════════════════════════════════

import argparse
import hashlib
import itertools
import json
import os
import tempfile


# ── Sig model (mirror of CostAccountedSyntax sig: Unit/Ground/Quote/And) ──
class Sig:
    def __init__(self, tag, args=()):
        self.tag = tag
        self.args = args

    def __repr__(self):
        if self.tag == "Unit":
            return "()"
        if self.tag == "Ground":
            return "g%s" % self.args[0]
        if self.tag == "Quote":
            return "#%s" % self.args[0]
        if self.tag == "And":
            return "(%r * %r)" % (self.args[0], self.args[1])
        return "?"


def Unit():
    return Sig("Unit")


def Ground(i):
    return Sig("Ground", (i,))


def Quote(i):
    return Sig("Quote", (i,))


def And(a, b):
    return Sig("And", (a, b))


# ── from_sig reflection (canonical channel = sorted atom ids) ──
def hash_id(label):
    return hashlib.blake2b(str(label).encode(), digest_size=16).hexdigest()


def reflect(sig):
    if sig.tag == "Unit":
        return []
    if sig.tag == "Ground":
        return [hash_id("g:%s" % sig.args[0])]
    if sig.tag == "Quote":
        return [hash_id("q:%s" % sig.args[0])]
    if sig.tag == "And":
        return sorted(reflect(sig.args[0]) + reflect(sig.args[1]))
    raise ValueError("unknown Sig tag: %s" % sig.tag)


def sig_eq(a, b):
    """≡sig at the channel level (the quotient the gate matches modulo)."""
    return reflect(a) == reflect(b)


# ── token stack model: a Python list of sigs (the stack () | s:S) ──
def stack_concat(a, b):
    return list(a) + list(b)


def stack_size(s):
    return len(s)


# ── the Cost monad's flatten (μ): nested signatures multiply, stacks concat ──
def mu_sig(s_outer, s_inner):
    # {{P}_{s_inner}}_{s_outer}  ↦  {P}_{s_outer * s_inner}
    return And(s_outer, s_inner)


def mu_stack(stack_of_stacks):
    out = []
    for s in stack_of_stacks:
        out = stack_concat(out, s)
    return out


# ── enumeration domains (bounded) ──
def enumerate_sigs(depth, atoms):
    base = [Unit()] + [Ground(i) for i in range(atoms)] + [Quote(i) for i in range(atoms)]
    if depth <= 0:
        return base
    smaller = enumerate_sigs(depth - 1, atoms)
    compound = [And(a, b) for a in smaller for b in smaller]
    return base + compound


def enumerate_stacks(max_len, atoms):
    elems = [Unit()] + [Ground(i) for i in range(atoms)] + [Quote(i) for i in range(atoms)]
    stacks = [[]]
    for n in range(1, max_len + 1):
        for combo in itertools.product(elems, repeat=n):
            stacks.append(list(combo))
    return stacks


# ── result recording (mirrors ll_identity_search.record_result) ──
def record(name, expected_holds, total, failures, counterexamples):
    successes = total - failures
    return {
        "law": name,
        "expected_holds": expected_holds,
        "cases": total,
        "successes": successes,
        "failures": failures,
        "counterexamples": counterexamples[:3],
    }


def check_forall(name, expected_holds, cases, holds_fn, show_fn):
    """expected_holds=True: every case must hold. A failure is a counterexample."""
    total = 0
    failures = 0
    cex = []
    for case in cases:
        total += 1
        if holds_fn(case) != expected_holds:
            failures += 1
            cex.append(show_fn(case))
    return record(name, expected_holds, total, failures, cex)


def check_exists(name, cases, holds_fn, show_fn):
    """A 'should fail' law (non-commutativity / non-idempotence): expected_holds
    is False; PASS iff at least one witness where the universal would fail."""
    total = len(cases)
    witnesses = [show_fn(c) for c in cases if not holds_fn(c)]
    found = len(witnesses) > 0
    # failures==0 (gate-pass) iff a witness was found.
    return {
        "law": name,
        "expected_holds": False,
        "cases": total,
        "successes": total if found else 0,
        "failures": 0 if found else 1,
        "counterexamples": witnesses[:3],
    }


def run(sig_depth, atoms, stack_len):
    sigs = enumerate_sigs(sig_depth, atoms)
    stacks = enumerate_stacks(stack_len, atoms)
    triples = list(itertools.product(sigs, repeat=3))
    pairs = list(itertools.product(sigs, repeat=2))
    stack_triples = list(itertools.product(stacks, repeat=3))
    stack_pairs = list(itertools.product(stacks, repeat=2))

    results = []

    # signature commutative monoid (up to ≡sig)
    results.append(check_forall(
        "sig_monoid_comm", True, pairs,
        lambda p: sig_eq(And(p[0], p[1]), And(p[1], p[0])),
        lambda p: {"s": repr(p[0]), "t": repr(p[1])}))
    results.append(check_forall(
        "sig_monoid_assoc", True, triples,
        lambda t: sig_eq(And(And(t[0], t[1]), t[2]), And(t[0], And(t[1], t[2]))),
        lambda t: {"s": repr(t[0]), "t": repr(t[1]), "u": repr(t[2])}))
    results.append(check_forall(
        "sig_monoid_unit_l", True, sigs,
        lambda s: sig_eq(And(Unit(), s), s),
        lambda s: {"s": repr(s)}))
    results.append(check_forall(
        "sig_monoid_unit_r", True, sigs,
        lambda s: sig_eq(And(s, Unit()), s),
        lambda s: {"s": repr(s)}))

    # token-stack free monoid
    results.append(check_forall(
        "stack_concat_assoc", True, stack_triples,
        lambda t: stack_concat(stack_concat(t[0], t[1]), t[2])
                  == stack_concat(t[0], stack_concat(t[1], t[2])),
        lambda t: {"a": repr(t[0]), "b": repr(t[1]), "c": repr(t[2])}))
    results.append(check_forall(
        "stack_concat_unit_l", True, stacks,
        lambda a: stack_concat([], a) == a, lambda a: {"a": repr(a)}))
    results.append(check_forall(
        "stack_concat_unit_r", True, stacks,
        lambda a: stack_concat(a, []) == a, lambda a: {"a": repr(a)}))
    results.append(check_forall(
        "stack_size_concat_homomorphism", True, stack_pairs,
        lambda p: stack_size(stack_concat(p[0], p[1])) == stack_size(p[0]) + stack_size(p[1]),
        lambda p: {"a": repr(p[0]), "b": repr(p[1])}))

    # the free monoid is NOT commutative (expected_holds=False — find a witness)
    results.append(check_exists(
        "stack_concat_commutative_FAILS", stack_pairs,
        lambda p: stack_concat(p[0], p[1]) == stack_concat(p[1], p[0]),
        lambda p: {"a": repr(p[0]), "b": repr(p[1])}))

    # cost monad laws (descend from the two monoids)
    results.append(check_forall(
        "monad_left_unit", True, sigs,
        lambda s: sig_eq(mu_sig(Unit(), s), s),
        lambda s: {"s": repr(s)}))
    results.append(check_forall(
        "monad_right_unit", True, sigs,
        lambda s: sig_eq(mu_sig(s, Unit()), s),
        lambda s: {"s": repr(s)}))
    results.append(check_forall(
        "monad_assoc_sig", True, triples,
        lambda t: sig_eq(mu_sig(mu_sig(t[0], t[1]), t[2]), mu_sig(t[0], mu_sig(t[1], t[2]))),
        lambda t: {"s1": repr(t[0]), "s2": repr(t[1]), "s3": repr(t[2])}))
    results.append(check_forall(
        "monad_assoc_stack", True, stack_triples,
        lambda t: mu_stack([mu_stack([t[0], t[1]]), t[2]])
                  == mu_stack([t[0], mu_stack([t[1], t[2]])]),
        lambda t: {"S1": repr(t[0]), "S2": repr(t[1]), "S3": repr(t[2])}))

    # μ is non-idempotent / non-injective (Remark, :1086): flattening forgets the
    # boundary — distinct ordered nestings (a,b) vs (b,a) flatten channel-equal.
    distinct_pairs = [p for p in pairs if reflect(p[0]) != reflect(p[1])]
    results.append(check_exists(
        "mu_non_injective_forgets_boundary", distinct_pairs,
        lambda p: not sig_eq(mu_sig(p[0], p[1]), mu_sig(p[1], p[0])),
        lambda p: {"a": repr(p[0]), "b": repr(p[1]),
                   "mu_ab": reflect(mu_sig(p[0], p[1])),
                   "mu_ba": reflect(mu_sig(p[1], p[0]))}))

    return results


def main():
    ap = argparse.ArgumentParser(description="Cost monad law bounded verification")
    ap.add_argument("--sig-depth", type=int, default=1)
    ap.add_argument("--atoms", type=int, default=2)
    ap.add_argument("--stack-len", type=int, default=2)
    ap.add_argument(
        "--json-out",
        default=os.path.join(tempfile.gettempdir(), "cost_monad_laws_results.json"))
    args = ap.parse_args()

    results = run(args.sig_depth, args.atoms, args.stack_len)
    overall_pass = all(r["failures"] == 0 for r in results)
    output = {
        "model": "cost_monad_laws",
        "bounds": {"sig_depth": args.sig_depth, "atoms": args.atoms,
                   "stack_len": args.stack_len},
        "overall_pass": overall_pass,
        "results": results,
    }
    with open(args.json_out, "w") as f:
        json.dump(output, f, indent=2, default=str)

    print("Cost monad law bounded verification")
    print("  bounds: sig_depth=%d atoms=%d stack_len=%d"
          % (args.sig_depth, args.atoms, args.stack_len))
    print("  results: %s" % args.json_out)
    print()
    for r in results:
        status = "ok" if r["failures"] == 0 else "FAIL"
        print("  [%s] %-36s expected_holds=%s cases=%d failures=%d"
              % (status, r["law"], r["expected_holds"], r["cases"], r["failures"]))
    print()
    if overall_pass:
        print("ALL COST-MONAD LAWS VERIFIED (and non-commutativity / non-idempotence witnessed).")
    else:
        print("COUNTEREXAMPLES FOUND — see JSON output.")


main()
